//! Runs the Rimelight Personal setup against a real Cloudflare account, reusing what already exists. Sign in when the
//! browser opens. Prints the backend's address and audience; never prints the token.
//!
//!   cargo run -p rimelight_personal_core --example provision_live -- <account-id> <worker-bundle.js> <schema.sql>

use std::fs;

use rimelight_personal_core::cf_oauth;
use rimelight_personal_core::cloudflare_api::BearerCloudflare;
use rimelight_personal_core::provision::{Plan, provision};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, account_id, bundle_path, schema_path, owner] = args.as_slice() else {
        eprintln!(
            "usage: provision_live <account-id> <worker-bundle.js> <schema.sql> <owner-email>"
        );
        std::process::exit(2);
    };

    println!("Opening the Cloudflare sign-in page...");
    let session = match cf_oauth::sign_in(|url| open::that(url).map_err(|e| e.to_string())) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("sign-in failed: {e}");
            std::process::exit(1);
        }
    };
    println!("Signed in.");

    let plan = Plan {
        account_id: account_id.clone(),
        owner_email: owner.clone(),
        bundle: fs::read(bundle_path).expect("read the worker bundle"),
        schema_sql: fs::read_to_string(schema_path).expect("read the schema"),
    };
    let cf = BearerCloudflare::new(session.token);

    match provision(&cf, &plan) {
        Ok(out) => {
            println!("api base:     {}", out.api_base);
            println!("access aud:   {}", out.access_aud);
            println!("team domain:  {}", out.team_domain);
            println!("database id:  {}", out.database_id);
        }
        Err(e) => {
            eprintln!("setup stopped: {e}");
            std::process::exit(1);
        }
    }
}
