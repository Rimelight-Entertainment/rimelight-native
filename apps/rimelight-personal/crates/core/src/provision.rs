//! Sets up one Rimelight Personal backend in the owner's Cloudflare account, through the Cloudflare API, with the
//! OAuth token from [`crate::cf_oauth`].
//!
//! The steps, in order: D1 database, schema, R2 bucket, Worker upload, workers.dev route, Access application for the
//! Worker's address, and a second Worker upload that carries the Access audience. [`provision`] runs them through the
//! [`Cloudflare`] trait, so the sequence is tested against a fake and never touches a real account in tests.
//!
//! Each step checks for what it would create first, so running setup again reuses what exists.

use serde_json::{Value, json};

pub const API_BASE: &str = "https://api.cloudflare.com/client/v4";
/// The Worker and resource name every owner's instance uses. It is per account, so there is no clash across owners.
pub const NAME: &str = "rimelight-personal";
/// The Worker's module name inside the upload.
pub const MODULE_NAME: &str = "fetch.js";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfMethod {
    Get,
    Post,
    Put,
    Patch,
}

/// The body of one API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    None,
    Json(Value),
    /// A Worker upload: the metadata part and the module, sent as `multipart/form-data`.
    WorkerUpload {
        metadata: Value,
        module: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CfError {
    /// The API answered with an error. `code` is Cloudflare's error code when it gave one.
    Api {
        status: u16,
        code: Option<u64>,
        message: String,
    },
    /// The request could not be sent or answered.
    Transport(String),
    /// The answer did not have the shape the step needs.
    Shape(String),
}

impl std::fmt::Display for CfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Api {
                status,
                code,
                message,
            } => match code {
                Some(c) => write!(f, "Cloudflare answered {status} ({c}): {message}"),
                None => write!(f, "Cloudflare answered {status}: {message}"),
            },
            Self::Transport(e) => write!(f, "could not reach Cloudflare: {e}"),
            Self::Shape(e) => write!(f, "unexpected answer from Cloudflare: {e}"),
        }
    }
}

/// One API call, relative to [`API_BASE`]. The real implementation sends it with the bearer token; tests use a fake.
pub trait Cloudflare {
    fn call(&self, method: CfMethod, path: &str, body: Body) -> Result<Value, CfError>;
}

/// What the setup needs from the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub account_id: String,
    /// The owner's email: the Access policy's only allowed identity, and the owner id stored on the rows.
    pub owner_email: String,
    /// The compiled Worker (the `fetch.js` module).
    pub bundle: Vec<u8>,
    /// The schema, as drizzle wrote it (statements separated by `--> statement-breakpoint`).
    pub schema_sql: String,
}

/// The result: where the backend answers, and the values the app stores with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provisioned {
    pub api_base: String,
    pub access_aud: String,
    pub team_domain: String,
    pub database_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionError {
    /// The Cloudflare account has no Zero Trust organisation yet, which the Access step needs. The owner creates it once
    /// in the dashboard; the app shows the step.
    NoAccessOrganisation,
    /// The account has no workers.dev subdomain yet. The owner sets it once in the dashboard.
    NoWorkersSubdomain,
    Api {
        step: &'static str,
        error: CfError,
    },
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoAccessOrganisation => write!(
                f,
                "This Cloudflare account has no Zero Trust team yet. Create one in the Cloudflare dashboard, then run setup again."
            ),
            Self::NoWorkersSubdomain => write!(
                f,
                "This Cloudflare account has no workers.dev subdomain yet. Set one in the Cloudflare dashboard, then run setup again."
            ),
            Self::Api { step, error } => write!(f, "{step} failed: {error}"),
        }
    }
}

fn account(plan: &Plan, rest: &str) -> String {
    format!("/accounts/{}{}", plan.account_id, rest)
}

/// Splits drizzle's migration into statements, dropping blank and comment-only chunks.
pub fn split_statements(sql: &str) -> Vec<String> {
    sql.split("--> statement-breakpoint")
        .map(|chunk| chunk.trim().to_owned())
        .filter(|chunk| {
            chunk
                .lines()
                .any(|line| !line.trim().is_empty() && !line.trim_start().starts_with("--"))
        })
        .collect()
}

/// The bindings the Worker needs. `policy_aud` is empty until the Access application exists, and a Worker with an
/// empty audience refuses every request, by design.
pub fn bindings(plan: &Plan, database_id: &str, policy_aud: &str, team_domain: &str) -> Value {
    json!([
        { "type": "d1", "name": "DB", "id": database_id },
        { "type": "r2_bucket", "name": "PERSONAL_BLOB", "bucket_name": NAME },
        { "type": "durable_object_namespace", "name": "MAILBOX", "class_name": "MailboxDO" },
        { "type": "send_email", "name": "EMAIL" },
        { "type": "plain_text", "name": "POLICY_AUD", "text": policy_aud },
        { "type": "plain_text", "name": "TEAM_DOMAIN", "text": team_domain },
        { "type": "plain_text", "name": "DOMAINS", "text": "" },
        { "type": "json", "name": "EMAIL_ADDRESSES", "json": [] },
        { "type": "secret_text", "name": "PERSONAL_OWNER_ID", "text": plan.owner_email },
    ])
}

/// The metadata of a Worker upload: the main module, the compatibility date, the bindings and the Durable Object
/// migration that creates `MailboxDO`.
pub fn worker_metadata(bindings: Value) -> Value {
    json!({
        "main_module": MODULE_NAME,
        "compatibility_date": "2026-10-01",
        "bindings": bindings,
        "migrations": { "new_tag": "v1", "new_sqlite_classes": ["MailboxDO"] },
        // Every provisioned Durable Object class must be declared, or the upload is refused.
        "exports": { "MailboxDO": { "type": "durable-object", "storage": "sqlite" } },
        "observability": { "enabled": true },
    })
}

/// Runs the setup against `cf`. Returns where the backend answers and the values the app stores.
pub fn provision(cf: &dyn Cloudflare, plan: &Plan) -> Result<Provisioned, ProvisionError> {
    let api = |step: &'static str| move |error: CfError| ProvisionError::Api { step, error };

    // 1. D1 database: reuse by name, otherwise create.
    let database_id = match find_by_name(cf, &account(plan, "/d1/database"), NAME)
        .map_err(api("finding the database"))?
    {
        Some(id) => id,
        None => {
            let created = cf
                .call(
                    CfMethod::Post,
                    &account(plan, "/d1/database"),
                    Body::Json(json!({ "name": NAME })),
                )
                .map_err(api("creating the database"))?;
            string_at(&created, &["uuid"]).map_err(api("creating the database"))?
        }
    };

    // 2. Schema: applied only to an empty database.
    let tables = cf
        .call(
            CfMethod::Post,
            &account(plan, &format!("/d1/database/{database_id}/query")),
            Body::Json(
                json!({ "sql": "SELECT count(*) AS n FROM sqlite_master WHERE type = 'table'" }),
            ),
        )
        .map_err(api("checking the schema"))?;
    if row_count(&tables) == 0 {
        for statement in split_statements(&plan.schema_sql) {
            cf.call(
                CfMethod::Post,
                &account(plan, &format!("/d1/database/{database_id}/query")),
                Body::Json(json!({ "sql": statement })),
            )
            .map_err(api("applying the schema"))?;
        }
    }

    // 3. R2 bucket: reuse by name, otherwise create.
    let buckets = cf
        .call(CfMethod::Get, &account(plan, "/r2/buckets"), Body::None)
        .map_err(api("finding the storage bucket"))?;
    if !has_bucket(&buckets, NAME) {
        cf.call(
            CfMethod::Post,
            &account(plan, "/r2/buckets"),
            Body::Json(json!({ "name": NAME })),
        )
        .map_err(api("creating the storage bucket"))?;
    }

    // 4. workers.dev subdomain: needed for the Worker's address.
    let subdomain = cf
        .call(
            CfMethod::Get,
            &account(plan, "/workers/subdomain"),
            Body::None,
        )
        .map_err(api("reading the workers.dev subdomain"))?;
    let subdomain =
        string_at(&subdomain, &["subdomain"]).map_err(|_| ProvisionError::NoWorkersSubdomain)?;
    let host = format!("{NAME}.{subdomain}.workers.dev");

    // 5. Worker, first upload: without an audience yet, so every request is refused until step 8. A Worker that
    // already exists keeps its Durable Object class, so the migration is sent only for a new Worker.
    let existed = worker_exists(cf, plan).map_err(api("finding the backend"))?;
    let first = bindings(plan, &database_id, "", "");
    upload_worker(cf, plan, first, !existed).map_err(api("uploading the backend"))?;

    // 6. Turn on the workers.dev route for it.
    cf.call(
        CfMethod::Post,
        &account(plan, &format!("/workers/scripts/{NAME}/subdomain")),
        Body::Json(json!({ "enabled": true, "previews_enabled": false })),
    )
    .map_err(api("turning on the workers.dev route"))?;

    // 7. Access: the team, then the application for the Worker's address with the owner as the only allowed identity.
    let org = cf
        .call(
            CfMethod::Get,
            &account(plan, "/access/organizations"),
            Body::None,
        )
        .map_err(api("reading the Zero Trust team"))?;
    let team_domain =
        string_at(&org, &["auth_domain"]).map_err(|_| ProvisionError::NoAccessOrganisation)?;

    // Reuse the application for this address when it exists; otherwise create it.
    let access_aud =
        match find_access_app(cf, plan, &host).map_err(api("finding the Access application"))? {
            Some(aud) => aud,
            None => {
                let app = cf
                    .call(
                        CfMethod::Post,
                        &account(plan, "/access/apps"),
                        Body::Json(json!({
                            "name": NAME,
                            "domain": host,
                            "type": "self_hosted",
                            "session_duration": "24h",
                            "policies": [{
                                "name": "owner only",
                                "decision": "allow",
                                "precedence": 1,
                                "include": [{ "email": { "email": plan.owner_email } }],
                                "exclude": [],
                                "require": [],
                            }],
                        })),
                    )
                    .map_err(api("creating the Access application"))?;
                string_at(&app, &["aud"]).map_err(api("creating the Access application"))?
            }
        };

    // 8. Worker, second upload: now with the audience and team, so the owner's sign-in is accepted.
    let second = bindings(
        plan,
        &database_id,
        &access_aud,
        &format!("https://{team_domain}"),
    );
    upload_worker(cf, plan, second, !existed).map_err(api("setting the backend's sign-in"))?;

    Ok(Provisioned {
        api_base: format!("https://{host}/api"),
        access_aud,
        team_domain: format!("https://{team_domain}"),
        database_id,
    })
}

fn upload_worker(
    cf: &dyn Cloudflare,
    plan: &Plan,
    bindings: Value,
    new_worker: bool,
) -> Result<(), CfError> {
    let mut metadata = worker_metadata(bindings);
    if !new_worker {
        // The Durable Object class already exists on this Worker; repeating its migration is an error.
        metadata.as_object_mut().map(|m| m.remove("migrations"));
    }
    cf.call(
        CfMethod::Put,
        &account(plan, &format!("/workers/scripts/{NAME}")),
        Body::WorkerUpload {
            metadata,
            module: plan.bundle.clone(),
        },
    )
    .map(|_| ())
}

/// Whether the Worker already exists on the account.
fn worker_exists(cf: &dyn Cloudflare, plan: &Plan) -> Result<bool, CfError> {
    match cf.call(
        CfMethod::Get,
        &account(plan, &format!("/workers/scripts/{NAME}")),
        Body::None,
    ) {
        Ok(_) => Ok(true),
        Err(CfError::Api { status: 404, .. }) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Whether the account already has an R2 bucket with this name.
fn has_bucket(answer: &Value, name: &str) -> bool {
    answer
        .get("result")
        .and_then(|r| r.get("buckets"))
        .and_then(Value::as_array)
        .is_some_and(|buckets| {
            buckets
                .iter()
                .any(|b| b.get("name").and_then(Value::as_str) == Some(name))
        })
}

/// The audience of the Access application for this address, if one exists.
fn find_access_app(
    cf: &dyn Cloudflare,
    plan: &Plan,
    host: &str,
) -> Result<Option<String>, CfError> {
    let list = cf.call(CfMethod::Get, &account(plan, "/access/apps"), Body::None)?;
    let apps = list
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(apps
        .iter()
        .find(|app| app.get("domain").and_then(Value::as_str) == Some(host))
        .and_then(|app| app.get("aud"))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// The id of the item named `name` in a list endpoint, if there is one.
fn find_by_name(cf: &dyn Cloudflare, path: &str, name: &str) -> Result<Option<String>, CfError> {
    let list = cf.call(CfMethod::Get, path, Body::None)?;
    let items = list
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        .and_then(|item| item.get("uuid").or_else(|| item.get("id")))
        .and_then(Value::as_str)
        .map(str::to_owned))
}

/// Reads a string from `result` (or from the answer itself) at the given path.
fn string_at(answer: &Value, path: &[&str]) -> Result<String, CfError> {
    let mut node = answer.get("result").unwrap_or(answer);
    for key in path {
        node = node
            .get(*key)
            .ok_or_else(|| CfError::Shape(format!("no {key} in the answer")))?;
    }
    node.as_str()
        .map(str::to_owned)
        .ok_or_else(|| CfError::Shape(format!("{} is not text", path.join("."))))
}

/// The number in the first row of a `count(*)` query, or 0 when the answer is empty.
fn row_count(answer: &Value) -> u64 {
    answer
        .get("result")
        .and_then(Value::as_array)
        .and_then(|sets| sets.first())
        .and_then(|set| set.get("results"))
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("n"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// The `multipart/form-data` body of a Worker upload: the metadata part, then the module. Returns the boundary too.
pub fn multipart_body(metadata: &Value, module: &[u8]) -> (String, Vec<u8>) {
    let boundary = "rimelight-personal-upload".to_owned();
    let mut body = Vec::new();
    body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    body.extend_from_slice(b"Content-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n");
    body.extend_from_slice(metadata.to_string().as_bytes());
    body.extend_from_slice(format!("\r\n--{boundary}\r\n").as_bytes());
    body.extend_from_slice(
        format!(
            "Content-Disposition: form-data; name=\"{MODULE_NAME}\"; filename=\"{MODULE_NAME}\"\r\nContent-Type: application/javascript+module\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(module);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A fake account: answers each call from a script and records what was asked.
    struct Fake {
        calls: RefCell<Vec<(CfMethod, String)>>,
        org: Option<&'static str>,
        subdomain: &'static str,
        schema_empty: bool,
        bucket_exists: bool,
        worker_exists: bool,
        db_exists: bool,
        /// The audience of an Access application already on the host, if any.
        existing_app_aud: Option<&'static str>,
    }

    impl Fake {
        fn new() -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                org: Some("team.cloudflareaccess.com"),
                subdomain: "owner",
                schema_empty: true,
                bucket_exists: false,
                worker_exists: false,
                db_exists: false,
                existing_app_aud: None,
            }
        }
    }

    impl Cloudflare for Fake {
        fn call(&self, method: CfMethod, path: &str, body: Body) -> Result<Value, CfError> {
            self.calls
                .borrow_mut()
                .push((method.clone(), path.to_owned()));
            let answer = match (method, path) {
                (CfMethod::Get, p) if p.ends_with("/d1/database") => {
                    let dbs = if self.db_exists {
                        json!([{ "name": NAME, "uuid": "db-1" }])
                    } else {
                        json!([])
                    };
                    json!({ "result": dbs })
                }
                (CfMethod::Post, p) if p.ends_with("/d1/database") => {
                    json!({ "result": { "uuid": "db-1" } })
                }
                (CfMethod::Post, p) if p.ends_with("/query") => {
                    let n = if self.schema_empty { 0 } else { 5 };
                    json!({ "result": [{ "results": [{ "n": n }] }] })
                }
                (CfMethod::Post, p) if p.ends_with("/r2/buckets") => json!({ "result": {} }),
                (CfMethod::Get, p) if p.ends_with("/workers/subdomain") => {
                    json!({ "result": { "subdomain": self.subdomain } })
                }
                (CfMethod::Put, p) if p.contains("/workers/scripts/") => {
                    if let Body::WorkerUpload { metadata, module } = &body {
                        assert_eq!(module.len() > 0, true, "the bundle is uploaded");
                        assert_eq!(metadata["main_module"], MODULE_NAME);
                    }
                    json!({ "result": {} })
                }
                (CfMethod::Post, p) if p.ends_with("/subdomain") => json!({ "result": {} }),
                (CfMethod::Get, p) if p.ends_with("/access/organizations") => match self.org {
                    Some(domain) => json!({ "result": { "auth_domain": domain } }),
                    None => json!({ "result": null }),
                },
                (CfMethod::Post, p) if p.ends_with("/access/apps") => {
                    json!({ "result": { "aud": "aud-123" } })
                }
                (CfMethod::Get, p) if p.ends_with("/r2/buckets") => {
                    let buckets = if self.bucket_exists {
                        json!([{ "name": NAME }])
                    } else {
                        json!([])
                    };
                    json!({ "result": { "buckets": buckets } })
                }
                (CfMethod::Get, p) if p.ends_with(&format!("/workers/scripts/{NAME}")) => {
                    if self.worker_exists {
                        json!({ "result": {} })
                    } else {
                        return Err(CfError::Api {
                            status: 404,
                            code: Some(10007),
                            message: "not found".to_owned(),
                        });
                    }
                }
                (CfMethod::Get, p) if p.ends_with("/access/apps") => {
                    let apps = match self.existing_app_aud {
                        Some(aud) => {
                            json!([{ "domain": format!("{NAME}.owner.workers.dev"), "aud": aud }])
                        }
                        None => json!([]),
                    };
                    json!({ "result": apps })
                }
                other => panic!("unexpected call {other:?}"),
            };
            Ok(answer)
        }
    }

    fn plan() -> Plan {
        Plan {
            account_id: "acct".to_owned(),
            owner_email: "owner@example.com".to_owned(),
            bundle: b"export default {};".to_vec(),
            schema_sql:
                "CREATE TABLE a (id int);\n--> statement-breakpoint\nCREATE TABLE b (id int);"
                    .to_owned(),
        }
    }

    #[test]
    fn a_fresh_account_gets_the_whole_backend_and_the_audience() {
        let fake = Fake::new();
        let out = provision(&fake, &plan()).expect("provisioned");
        assert_eq!(
            out.api_base,
            "https://rimelight-personal.owner.workers.dev/api"
        );
        assert_eq!(out.access_aud, "aud-123");
        assert_eq!(out.team_domain, "https://team.cloudflareaccess.com");
        assert_eq!(out.database_id, "db-1");
        // Two Worker uploads: one before the audience exists, one after.
        let uploads = fake
            .calls
            .borrow()
            .iter()
            .filter(|(m, p)| *m == CfMethod::Put && p.contains("/workers/scripts/"))
            .count();
        assert_eq!(uploads, 2);
    }

    #[test]
    fn an_existing_schema_is_not_applied_again() {
        let fake = Fake {
            schema_empty: false,
            ..Fake::new()
        };
        provision(&fake, &plan()).expect("provisioned");
        let applied = fake
            .calls
            .borrow()
            .iter()
            .filter(|(_, p)| p.ends_with("/query"))
            .count();
        assert_eq!(applied, 1, "only the schema check runs");
    }

    #[test]
    fn an_existing_instance_is_reused_not_created_again() {
        let fake = Fake {
            schema_empty: false,
            bucket_exists: true,
            worker_exists: true,
            db_exists: true,
            existing_app_aud: Some("aud-existing"),
            ..Fake::new()
        };
        let out = provision(&fake, &plan()).expect("provisioned");
        assert_eq!(out.access_aud, "aud-existing");
        let calls = fake.calls.borrow();
        assert!(
            !calls
                .iter()
                .any(|(m, p)| *m == CfMethod::Post && p.ends_with("/r2/buckets")),
            "no new bucket"
        );
        assert!(
            !calls
                .iter()
                .any(|(m, p)| *m == CfMethod::Post && p.ends_with("/access/apps")),
            "no new Access app"
        );
        assert!(
            !calls
                .iter()
                .any(|(m, p)| *m == CfMethod::Post && p.ends_with("/d1/database")),
            "no new database"
        );
    }

    #[test]
    fn an_existing_worker_is_uploaded_without_its_migration() {
        struct Recorder(RefCell<Vec<Value>>);
        impl Cloudflare for Recorder {
            fn call(&self, _: CfMethod, _: &str, body: Body) -> Result<Value, CfError> {
                if let Body::WorkerUpload { metadata, .. } = body {
                    self.0.borrow_mut().push(metadata);
                }
                Ok(json!({ "result": {} }))
            }
        }
        let rec = Recorder(RefCell::new(Vec::new()));
        upload_worker(&rec, &plan(), json!([]), false).expect("uploaded");
        upload_worker(&rec, &plan(), json!([]), true).expect("uploaded");
        let uploads = rec.0.borrow();
        assert!(
            uploads[0].get("migrations").is_none(),
            "existing Worker: no migration"
        );
        assert!(
            uploads[1].get("migrations").is_some(),
            "new Worker: migration creates MailboxDO"
        );
    }

    #[test]
    fn a_missing_team_stops_with_the_dashboard_step() {
        let fake = Fake {
            org: None,
            ..Fake::new()
        };
        assert_eq!(
            provision(&fake, &plan()),
            Err(ProvisionError::NoAccessOrganisation)
        );
    }

    #[test]
    fn statements_split_on_drizzle_breakpoints() {
        let parts = split_statements(
            "-- note\nCREATE TABLE a (id int);\n--> statement-breakpoint\n-- only a comment\n",
        );
        assert_eq!(parts, vec!["-- note\nCREATE TABLE a (id int);".to_owned()]);
    }

    #[test]
    fn bindings_carry_the_owner_as_a_secret_and_the_audience() {
        let value = bindings(&plan(), "db-1", "aud-123", "https://team.example");
        let by_name = |name: &str| {
            value
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["name"] == name)
                .cloned()
                .unwrap()
        };
        assert_eq!(by_name("PERSONAL_OWNER_ID")["type"], "secret_text");
        assert_eq!(by_name("PERSONAL_OWNER_ID")["text"], "owner@example.com");
        assert_eq!(by_name("POLICY_AUD")["text"], "aud-123");
        assert_eq!(by_name("DB")["id"], "db-1");
        assert_eq!(by_name("MAILBOX")["class_name"], "MailboxDO");
    }

    #[test]
    fn the_upload_names_the_module_and_carries_the_bytes() {
        let (boundary, body) =
            multipart_body(&json!({ "main_module": MODULE_NAME }), b"console.log(1)");
        let text = String::from_utf8_lossy(&body);
        assert!(text.starts_with(&format!("--{boundary}")));
        assert!(text.contains("name=\"fetch.js\""));
        assert!(text.contains("console.log(1)"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }
}
