//! Unit tests for the inbox controller, against a fake API and a fake shell. No network and no window.

use super::*;
use rimelight_personal_core::inbox::{Attachment, MailboxSettings, Signature};

/// Records each call as `name:args` and answers from the seeded data. `fail` makes the writes fail.
#[derive(Default)]
struct Fake {
    calls: Mutex<Vec<String>>,
    mailboxes: Mutex<Vec<Mailbox>>,
    folders: Mutex<Vec<Folder>>,
    emails: Mutex<Vec<Email>>,
    thread: Mutex<Vec<Email>>,
    fail: Mutex<bool>,
    /// The HTML of the last message sent or saved.
    last_html: Mutex<String>,
    /// Bytes every download returns, and whether downloads fail.
    fetch_bytes: Mutex<Vec<u8>>,
    fetch_fail: Mutex<bool>,
}

impl Fake {
    fn record(&self, call: String) {
        self.calls.lock().unwrap().push(call);
    }

    fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn write_result(&self) -> Result<(), ApiError> {
        if *self.fail.lock().unwrap() {
            Err(ApiError::Status {
                code: 500,
                body: "boom".to_owned(),
            })
        } else {
            Ok(())
        }
    }

    fn set_html(&self, html: &str) {
        *self.last_html.lock().unwrap() = html.to_owned();
    }
}

impl InboxApi for Arc<Fake> {
    fn config(&self, on_done: Done<Config>) {
        self.record("config".to_owned());
        on_done(Ok(Config {
            domains: vec!["example.com".to_owned()],
        }));
    }

    fn mailboxes(&self, on_done: Done<Vec<Mailbox>>) {
        self.record("mailboxes".to_owned());
        on_done(Ok(self.mailboxes.lock().unwrap().clone()));
    }

    fn mailbox(&self, id: &str, on_done: Done<Mailbox>) {
        self.record(format!("mailbox:{id}"));
        let mailbox = Mailbox {
            id: id.to_owned(),
            email: "daniel@example.com".to_owned(),
            name: "Daniel".to_owned(),
            settings: Some(MailboxSettings {
                from_name: Some("Daniel".to_owned()),
                signature: Some(Signature {
                    enabled: true,
                    text: "-- D".to_owned(),
                }),
            }),
        };
        on_done(Ok(mailbox));
    }

    fn create_mailbox(&self, email: &str, name: &str, on_done: Done<()>) {
        self.record(format!("create_mailbox:{email}:{name}"));
        on_done(self.write_result());
    }

    fn update_mailbox(&self, id: &str, patch: &MailboxSettingsPatch<'_>, on_done: Done<()>) {
        self.record(format!("update_mailbox:{id}:{}", patch.settings.from_name));
        on_done(self.write_result());
    }

    fn delete_mailbox(&self, id: &str, on_done: Done<()>) {
        self.record(format!("delete_mailbox:{id}"));
        on_done(self.write_result());
    }

    fn folders(&self, mailbox: &str, on_done: Done<Vec<Folder>>) {
        self.record(format!("folders:{mailbox}"));
        on_done(Ok(self.folders.lock().unwrap().clone()));
    }

    fn create_folder(&self, mailbox: &str, name: &str, on_done: Done<Folder>) {
        self.record(format!("create_folder:{mailbox}:{name}"));
        let folder = Folder {
            id: "f-new".to_owned(),
            name: name.to_owned(),
            unread_count: 0,
        };
        on_done(self.write_result().map(|()| folder));
    }

    fn update_folder(&self, mailbox: &str, folder_id: &str, name: &str, on_done: Done<Folder>) {
        self.record(format!("update_folder:{mailbox}:{folder_id}:{name}"));
        let folder = Folder {
            id: folder_id.to_owned(),
            name: name.to_owned(),
            unread_count: 0,
        };
        on_done(self.write_result().map(|()| folder));
    }

    fn delete_folder(&self, mailbox: &str, folder_id: &str, on_done: Done<()>) {
        self.record(format!("delete_folder:{mailbox}:{folder_id}"));
        on_done(self.write_result());
    }

    fn emails(&self, mailbox: &str, folder: &str, page: u32, on_done: Done<EmailPage>) {
        self.record(format!("emails:{mailbox}:{folder}:{page}"));
        let emails = self.emails.lock().unwrap().clone();
        let total_count = emails.len() as u32;
        on_done(Ok(EmailPage {
            emails,
            total_count,
        }));
    }

    fn search(&self, mailbox: &str, params: &SearchParams, page: u32, on_done: Done<EmailPage>) {
        self.record(format!("search:{mailbox}:{}:{page}", params.from));
        let emails = self.emails.lock().unwrap().clone();
        let total_count = emails.len() as u32;
        on_done(Ok(EmailPage {
            emails,
            total_count,
        }));
    }

    fn thread(&self, mailbox: &str, thread_id: &str, on_done: Done<Vec<Email>>) {
        self.record(format!("thread:{mailbox}:{thread_id}"));
        on_done(Ok(self.thread.lock().unwrap().clone()));
    }

    fn mark_thread_read(&self, _mailbox: &str, thread_id: &str, on_done: Done<()>) {
        self.record(format!("mark_read:{thread_id}"));
        on_done(Ok(()));
    }

    fn patch_email(&self, _mailbox: &str, id: &str, patch: &EmailPatch, on_done: Done<()>) {
        self.record(format!("patch:{id}:{:?}:{:?}", patch.read, patch.starred));
        on_done(self.write_result());
    }

    fn move_email(&self, _mailbox: &str, id: &str, folder_id: &str, on_done: Done<()>) {
        self.record(format!("move:{id}:{folder_id}"));
        on_done(self.write_result());
    }

    fn delete_email(&self, _mailbox: &str, id: &str, on_done: Done<()>) {
        self.record(format!("delete:{id}"));
        on_done(self.write_result());
    }

    fn send_email(&self, _mailbox: &str, message: &SendEmail, on_done: Done<()>) {
        self.record(format!("send:{}:{}", message.to, message.subject));
        self.set_html(&message.html);
        on_done(self.write_result());
    }

    fn reply(&self, _mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>) {
        self.record(format!("reply:{original_id}:{}", message.to));
        self.set_html(&message.html);
        on_done(self.write_result());
    }

    fn forward(&self, _mailbox: &str, original_id: &str, message: &SendEmail, on_done: Done<()>) {
        self.record(format!("forward:{original_id}:{}", message.subject));
        self.set_html(&message.html);
        on_done(self.write_result());
    }

    fn send_draft(&self, _mailbox: &str, draft_id: &str, draft: &DraftSend<'_>, on_done: Done<()>) {
        self.record(format!("send_draft:{draft_id}:{}", draft.subject));
        self.set_html(draft.html);
        on_done(self.write_result());
    }

    fn save_draft(&self, _mailbox: &str, draft: &DraftBody<'_>, on_done: Done<DraftSaved>) {
        self.record(format!("save_draft:{}", draft.draft_id.unwrap_or("none")));
        self.set_html(draft.body);
        on_done(self.write_result().map(|()| DraftSaved {
            id: "d1".to_owned(),
        }));
    }

    fn attachment_url(&self, _mailbox: &str, email_id: &str, attachment_id: &str) -> String {
        self.record(format!("attachment:{email_id}:{attachment_id}"));
        "https://api.test/att".to_owned()
    }

    fn fetch(&self, url: &str, on_done: Done<Vec<u8>>) {
        self.record(format!("fetch:{url}"));
        if *self.fetch_fail.lock().unwrap() {
            on_done(Err(ApiError::Transport("offline".to_owned())));
        } else {
            on_done(Ok(self.fetch_bytes.lock().unwrap().clone()));
        }
    }
}

/// Records the addresses opened, answers the file dialogs, and records what was saved.
#[derive(Default)]
struct FakeShell {
    opened: Mutex<Vec<String>>,
    picks: Mutex<Vec<PathBuf>>,
    /// Where the save dialog puts a file (`None` when the person cancels).
    save_target: Mutex<Option<PathBuf>>,
    refuse: Mutex<bool>,
}

impl Shell for Arc<FakeShell> {
    fn open(&self, target: &str) -> bool {
        self.opened.lock().unwrap().push(target.to_owned());
        !*self.refuse.lock().unwrap()
    }

    fn pick_files(&self) -> Vec<PathBuf> {
        self.picks.lock().unwrap().clone()
    }

    fn save_file(&self, _name: &str) -> Option<PathBuf> {
        self.save_target.lock().unwrap().clone()
    }
}

fn mail(id: &str, thread: &str, subject: &str, read: bool) -> Email {
    Email {
        id: id.to_owned(),
        folder_id: "inbox".to_owned(),
        subject: subject.to_owned(),
        sender: "ann@example.com".to_owned(),
        recipient: "daniel@example.com".to_owned(),
        cc: None,
        bcc: None,
        date: "2026-11-20T10:00:00.000Z".to_owned(),
        read,
        starred: false,
        body: "<p>Hello <a href=\"https://a.test\">site</a></p><script>x()</script>".to_owned(),
        snippet: "Hello".to_owned(),
        message_id: Some(format!("<{id}@mail>")),
        in_reply_to: None,
        thread_id: Some(thread.to_owned()),
        raw_headers: None,
        attachments: Vec::new(),
    }
}

fn test_dir() -> PathBuf {
    std::env::temp_dir().join(format!("rimelight-personal-inbox-test-{}", std::process::id()))
}

/// What setup() returns: the fake API, the fake shell, the controller and the views it published.
type Setup = (
    Arc<Fake>,
    Arc<FakeShell>,
    InboxController,
    Arc<Mutex<Vec<InboxView>>>,
);

/// A controller over the seeded fake: one mailbox, one custom folder, two messages in one thread and one thread.
fn setup() -> Setup {
    let fake: Arc<Fake> = Arc::default();
    *fake.mailboxes.lock().unwrap() = vec![Mailbox {
        id: "m1".to_owned(),
        email: "daniel@example.com".to_owned(),
        name: "Daniel".to_owned(),
        settings: None,
    }];
    *fake.folders.lock().unwrap() = vec![
        Folder {
            id: "inbox".to_owned(),
            name: "inbox".to_owned(),
            unread_count: 1,
        },
        Folder {
            id: "f-projects".to_owned(),
            name: "Projects".to_owned(),
            unread_count: 0,
        },
    ];
    *fake.emails.lock().unwrap() = vec![
        mail("a1", "t1", "Hello", false),
        mail("a2", "t1", "Hello", true),
    ];
    *fake.thread.lock().unwrap() = vec![
        mail("a1", "t1", "Hello", false),
        mail("a2", "t1", "Hello", true),
    ];
    let shell: Arc<FakeShell> = Arc::default();
    let seen: Arc<Mutex<Vec<InboxView>>> = Arc::default();
    let sink = seen.clone();
    let api: Arc<dyn InboxApi> = Arc::new(fake.clone());
    let shell_dyn: Arc<dyn Shell> = Arc::new(shell.clone());
    let controller = InboxController::new(api, shell_dyn, test_dir(), move |view| {
        sink.lock().unwrap().push(view)
    });
    (fake, shell, controller, seen)
}

fn loaded() -> (Arc<Fake>, Arc<FakeShell>, InboxController) {
    let (fake, shell, c, _) = setup();
    c.load();
    (fake, shell, c)
}

/// Like `loaded`, but the one message carries an attachment before the controller reads the list.
fn loaded_with_attachment(
    filename: &str,
    mimetype: &str,
    bytes: &[u8],
) -> (Arc<Fake>, Arc<FakeShell>, InboxController) {
    let (fake, shell, c, _) = setup();
    with_attachment(&fake, filename, mimetype, bytes);
    c.load();
    (fake, shell, c)
}

#[test]
fn load_reads_config_mailboxes_folders_and_the_first_page() {
    let (fake, _, c) = loaded();
    assert_eq!(
        fake.calls(),
        ["config", "mailboxes", "folders:m1", "emails:m1:inbox:1"]
    );
    let view = c.view();
    assert_eq!(view.mailboxes.len(), 1);
    assert!(view.mailboxes[0].selected);
    assert_eq!(view.folders[0].label, "Inbox");
    assert_eq!(view.folders[1].label, "Projects");
    assert!(view.folders[1].custom);
    assert_eq!(view.list.len(), 2);
    assert_eq!(view.list[0].subject, "Hello");
    assert_eq!(view.pages, 1);
    assert!(view.move_targets.iter().any(|(id, _)| id == "f-projects"));
}

#[test]
fn opening_a_thread_loads_it_and_marks_it_read() {
    let (fake, _, c) = loaded();
    c.select_thread("t1");
    assert!(fake.calls().contains(&"thread:m1:t1".to_owned()));
    assert!(fake.calls().contains(&"mark_read:t1".to_owned()));
    let view = c.view();
    assert_eq!(view.thread_key, "t1");
    assert_eq!(view.thread.len(), 2);
    assert!(view.thread.iter().all(|m| m.read));
    assert!(view.list.iter().all(|row| !row.unread));
    // The body is structured text, with the script removed and the link listed.
    let kinds: Vec<u8> = view.thread[0].lines.iter().map(|l| l.kind).collect();
    assert_eq!(kinds, vec![0, 2]);
    assert_eq!(view.thread[0].lines[0].text, "Hello site");
    assert_eq!(view.thread[0].lines[1].href, "https://a.test");
    c.clear_selection();
    assert!(c.view().thread.is_empty());
}

#[test]
fn search_uses_the_search_endpoint_and_pages_are_range_checked() {
    let (fake, _, c) = loaded();
    c.search("from:ann");
    assert!(fake.calls().contains(&"search:m1:ann:1".to_owned()));
    assert_eq!(c.view().search, "from:ann");
    assert!(!c.go_page(2), "one page of results");
    assert!(!c.go_page(0));
}

#[test]
fn star_and_read_send_the_flag_and_update_the_list() {
    let (fake, _, c) = loaded();
    c.set_starred("a1", true);
    assert!(
        fake.calls()
            .contains(&"patch:a1:None:Some(true)".to_owned())
    );
    let starred = c.view().list.iter().find(|r| r.id == "a1").unwrap().starred;
    assert!(starred);
    c.set_read("a1", true);
    assert!(
        fake.calls()
            .contains(&"patch:a1:Some(true):None".to_owned())
    );
    assert!(!c.view().list.iter().find(|r| r.id == "a1").unwrap().unread);
}

#[test]
fn move_sends_the_target_folder_and_refreshes() {
    let (fake, _, c) = loaded();
    c.move_message("a1", "archive");
    assert!(fake.calls().contains(&"move:a1:archive".to_owned()));
    assert!(
        fake.calls()
            .iter()
            .filter(|call| call.starts_with("emails:"))
            .count()
            >= 2
    );
}

#[test]
fn failed_write_shows_the_server_reason_in_the_banner() {
    let (fake, _, c) = loaded();
    *fake.fail.lock().unwrap() = true;
    c.set_starred("a1", true);
    let banner = c.view().banner;
    assert!(banner.contains("Could not update the message"), "{banner}");
    assert!(banner.contains("HTTP 500: boom"), "{banner}");
    c.dismiss_banner();
    assert!(c.view().banner.is_empty());
}

#[test]
fn creating_a_mailbox_checks_the_prefix_and_sends_the_address() {
    let (fake, _, c) = loaded();
    c.open_create();
    c.submit_create();
    assert!(c.view().create.unwrap().error.contains("before the @"));
    c.create_field(0, "a@b");
    c.submit_create();
    assert!(c.view().create.unwrap().error.contains("before the @"));
    c.create_field(0, "news");
    c.create_field(1, "News");
    c.submit_create();
    assert!(
        fake.calls()
            .contains(&"create_mailbox:news@example.com:News".to_owned())
    );
    assert!(
        c.view().create.is_none(),
        "a created mailbox closes the form"
    );
}

#[test]
fn settings_load_then_save_the_sender_name() {
    let (fake, _, c) = loaded();
    assert!(c.open_settings());
    let form = c.view().settings.unwrap();
    assert_eq!(form.from_name, "Daniel");
    assert_eq!(form.signature, "-- D");
    assert!(form.signature_enabled);
    c.settings_field(0, "Dan");
    c.save_settings();
    assert!(fake.calls().contains(&"update_mailbox:m1:Dan".to_owned()));
    assert!(c.view().settings.is_none());
}

#[test]
fn folders_are_created_renamed_and_refused_when_blank() {
    let (fake, _, c) = loaded();
    c.open_folder_create();
    c.submit_folder();
    assert_eq!(c.view().folder_form.unwrap().error, "Name the folder");
    c.folder_name("  Travel ");
    c.submit_folder();
    assert!(fake.calls().contains(&"create_folder:m1:Travel".to_owned()));
    assert!(c.view().folder_form.is_none());

    assert!(c.open_folder_rename("f-projects"));
    assert_eq!(c.view().folder_form.unwrap().name, "Projects");
    c.folder_name("Work");
    c.submit_folder();
    assert!(
        fake.calls()
            .contains(&"update_folder:m1:f-projects:Work".to_owned())
    );
}

#[test]
fn deleting_a_folder_waits_for_confirmation() {
    let (fake, _, c) = loaded();
    c.ask_delete_folder("f-projects");
    assert!(c.view().confirm.is_some());
    c.confirm_no();
    assert!(c.view().confirm.is_none());
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| call.starts_with("delete_folder"))
    );
    c.ask_delete_folder("f-projects");
    c.confirm_yes();
    assert!(
        fake.calls()
            .contains(&"delete_folder:m1:f-projects".to_owned())
    );
}

#[test]
fn deleting_the_mailbox_waits_for_confirmation() {
    let (fake, _, c) = loaded();
    assert!(c.ask_delete_mailbox());
    assert_eq!(c.view().confirm.unwrap().title, "Delete mailbox");
    c.confirm_yes();
    assert!(fake.calls().contains(&"delete_mailbox:m1".to_owned()));
}

#[test]
fn reply_all_prefills_the_sender_and_subject_and_sends_to_the_original() {
    let (fake, _, c) = loaded();
    assert!(c.reply("a1", true));
    let compose = c.view().compose.unwrap();
    assert_eq!(compose.title, "Reply all");
    assert_eq!(compose.to, "ann@example.com");
    assert_eq!(compose.subject, "Re: Hello");
    c.compose_field(FIELD_BODY, "Thanks");
    c.send();
    assert!(
        fake.calls()
            .contains(&"reply:a1:ann@example.com".to_owned())
    );
    assert!(c.view().compose.is_none());
}

#[test]
fn forward_keeps_one_fwd_prefix_and_uses_the_forward_endpoint() {
    let (fake, _, c) = loaded();
    assert!(c.forward("a1"));
    let compose = c.view().compose.unwrap();
    assert_eq!(compose.title, "Forward");
    assert_eq!(compose.subject, "Fwd: Hello");
    c.compose_field(FIELD_TO, "bob@example.com");
    c.send();
    assert!(fake.calls().contains(&"forward:a1:Fwd: Hello".to_owned()));
}

#[test]
fn a_new_message_is_sent_as_html_built_from_the_markup() {
    let (fake, _, c) = loaded();
    c.new_message();
    c.compose_field(FIELD_TO, "x@y.z");
    c.compose_field(FIELD_SUBJECT, "Hi");
    c.compose_field(FIELD_BODY, "**Hello** there");
    c.send();
    assert!(fake.calls().contains(&"send:x@y.z:Hi".to_owned()));
    assert_eq!(
        *fake.last_html.lock().unwrap(),
        "<p><strong>Hello</strong> there</p>"
    );
}

#[test]
fn sending_without_a_recipient_is_refused() {
    let (fake, _, c) = loaded();
    c.new_message();
    c.compose_field(FIELD_BODY, "no one to read this");
    c.send();
    assert_eq!(c.view().compose.unwrap().error, "Add a recipient");
    assert!(!fake.calls().iter().any(|call| call.starts_with("send")));
}

#[test]
fn the_draft_saves_after_the_typing_pause_and_then_sends_through_the_draft_endpoint() {
    let (fake, _, c) = loaded();
    c.new_message();
    c.compose_field(FIELD_TO, "x@y.z");
    c.compose_field(FIELD_BODY, "draft text");
    let now = now_secs();
    c.tick_at(now);
    assert!(
        !fake
            .calls()
            .iter()
            .any(|call| call.starts_with("save_draft")),
        "still typing"
    );
    c.tick_at(now + AUTOSAVE_DELAY + 1.0);
    assert!(fake.calls().contains(&"save_draft:none".to_owned()));
    assert_eq!(c.view().compose.unwrap().status, "Draft saved");
    c.send();
    assert!(fake.calls().contains(&"send_draft:d1:".to_owned()));
}

#[test]
fn attachments_come_from_the_file_dialog_and_show_their_size() {
    let (_, shell, c) = loaded();
    let path = test_dir().join("note.txt");
    std::fs::create_dir_all(test_dir()).unwrap();
    std::fs::write(&path, b"0123456789").unwrap();
    *shell.picks.lock().unwrap() = vec![path];
    c.new_message();
    assert_eq!(c.compose_attach(), 1);
    let compose = c.view().compose.unwrap();
    assert_eq!(compose.attachments, vec!["note.txt · 10 B".to_owned()]);
    assert!(c.compose_remove_attachment(0));
    assert!(c.view().compose.unwrap().attachments.is_empty());
}

#[test]
fn markup_buttons_append_their_markers() {
    let (_, _, c) = loaded();
    c.new_message();
    assert!(c.compose_markup("bold"));
    assert!(c.compose_markup("list"));
    assert!(!c.compose_markup("underline"));
    assert_eq!(c.view().compose.unwrap().body, "**bold text**\n- item");
}

#[test]
fn view_original_writes_a_safe_copy_and_opens_it() {
    let (_, shell, c) = loaded();
    assert!(c.view_original("a1"));
    let opened = shell.opened.lock().unwrap().clone();
    assert_eq!(opened.len(), 1);
    // The plain local path, not a file:/// address: ShellExecuteW opens paths.
    assert!(!opened[0].starts_with("file:"), "{}", opened[0]);
    assert!(opened[0].contains("rimelight-personal-inbox-test"), "{}", opened[0]);
    let text = std::fs::read_to_string(&opened[0]).unwrap();
    assert!(text.contains("Content-Security-Policy"));
    assert!(!text.contains("<script"));
    assert!(text.contains("<a href=\"https://a.test\">site</a>"));
}

/// Gives message `a1` one attachment and makes downloads return `bytes`.
fn with_attachment(fake: &Fake, filename: &str, mimetype: &str, bytes: &[u8]) {
    let att = Attachment {
        id: "att1".to_owned(),
        email_id: "a1".to_owned(),
        filename: filename.to_owned(),
        mimetype: mimetype.to_owned(),
        size: bytes.len() as u64,
        content_id: None,
        disposition: None,
    };
    // The same message sits in both lists; `find_email` checks the thread first.
    fake.emails.lock().unwrap()[0].attachments = vec![att.clone()];
    fake.thread.lock().unwrap()[0].attachments = vec![att];
    *fake.fetch_bytes.lock().unwrap() = bytes.to_vec();
}

#[test]
fn a_text_attachment_opens_in_the_app_viewer() {
    let (fake, _, c) = loaded_with_attachment("notes.txt", "text/plain", b"hello there");
    assert!(c.open_attachment("a1", "att1"));
    assert!(
        fake.calls()
            .contains(&"fetch:https://api.test/att".to_owned())
    );
    let viewer = c.view().viewer.expect("viewer open");
    assert_eq!(viewer.kind, 1);
    assert_eq!(viewer.text, "hello there");
    c.close_viewer();
    assert!(c.view().viewer.is_none());
}

#[test]
fn an_other_attachment_is_saved_then_opened_by_plain_path() {
    let (fake, shell, c) = loaded_with_attachment("plan.pdf", "application/pdf", b"%PDF-1.4");
    let target = test_dir().join("saved").join("plan.pdf");
    *shell.save_target.lock().unwrap() = Some(target.clone());
    assert!(c.open_attachment("a1", "att1"));
    assert_eq!(std::fs::read(&target).unwrap(), b"%PDF-1.4");
    assert_eq!(
        shell.opened.lock().unwrap().last().unwrap(),
        &target.to_string_lossy().into_owned()
    );
}

#[test]
fn a_cancelled_save_leaves_nothing_behind() {
    let (fake, shell, c) = loaded_with_attachment("plan.pdf", "application/pdf", b"%PDF-1.4");
    *shell.save_target.lock().unwrap() = None;
    assert!(c.open_attachment("a1", "att1"));
    assert!(shell.opened.lock().unwrap().is_empty());
    assert!(c.view().banner.is_empty());
}

#[test]
fn a_failed_download_opens_the_address_in_the_browser() {
    let (fake, shell, c) = loaded_with_attachment("plan.pdf", "application/pdf", b"");
    *fake.fetch_fail.lock().unwrap() = true;
    assert!(c.open_attachment("a1", "att1"));
    assert_eq!(shell.opened.lock().unwrap()[0], "https://api.test/att");
    assert!(c.view().banner.contains("Opened in the browser"));
}

#[test]
fn an_unreadable_picture_says_so_in_the_banner() {
    let (fake, _, c) = loaded_with_attachment("photo.png", "image/png", b"not a picture");
    assert!(c.open_attachment("a1", "att1"));
    assert!(c.view().banner.contains("Could not show photo.png"));
    assert!(c.view().viewer.is_none());
}

#[test]
fn data_pictures_in_mail_are_decoded_at_once_and_remote_ones_are_fetched() {
    let (fake, _, c) = loaded();
    // A 1x1 transparent PNG as a data URL.
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";
    let body = format!(
        "<p>Hi</p><img src=\"data:image/png;base64,{png}\" alt=\"dot\"><img src=\"https://img.test/x.png\" alt=\"remote\">"
    );
    fake.thread.lock().unwrap()[0].body = body;
    *fake.fetch_bytes.lock().unwrap() = Vec::new();
    *fake.fetch_fail.lock().unwrap() = true;
    c.select_thread("t1");
    let calls = fake.calls();
    assert!(calls.contains(&"fetch:https://img.test/x.png".to_owned()));
    let view = c.view();
    assert!(
        view.images.iter().any(|i| i.w == 1 && i.h == 1),
        "data picture decoded"
    );
}

#[test]
fn headers_open_with_the_sender_first_and_copy_by_row() {
    let (_, _, c) = loaded();
    assert!(c.open_headers("a1"));
    let headers = c.view().headers.unwrap();
    assert_eq!(headers[0].key, "From");
    assert_eq!(c.header_value(0).as_deref(), Some("ann@example.com"));
    assert_eq!(c.header_value(99), None);
    c.close_headers();
    assert!(c.view().headers.is_none());
}

#[test]
fn compose_subject_and_forward_subject_never_double_the_prefix() {
    assert_eq!(compose_subject("Hello"), "Re: Hello");
    assert_eq!(compose_subject("re: Hello"), "re: Hello");
    assert_eq!(forward_subject("Hello"), "Fwd: Hello");
    assert_eq!(forward_subject("FWD: Hello"), "FWD: Hello");
}

#[test]
fn attachment_kinds_and_safe_names() {
    assert!(matches!(
        attachment_kind("a.bin", "image/jpeg"),
        AttachmentKind::Image
    ));
    assert!(matches!(
        attachment_kind("clip.MOV", "application/octet-stream"),
        AttachmentKind::Video
    ));
    assert!(matches!(
        attachment_kind("memo.m4a", ""),
        AttachmentKind::Audio
    ));
    assert!(matches!(
        attachment_kind("log.txt", ""),
        AttachmentKind::Text
    ));
    assert!(matches!(
        attachment_kind("plan.pdf", "application/pdf"),
        AttachmentKind::Other
    ));
    assert_eq!(safe_file_name("../../etc/passwd"), "_.._etc_passwd");
    assert_eq!(safe_file_name("..hidden"), "hidden");
    assert_eq!(safe_file_name(""), "attachment");
}

#[test]
fn base64_decodes_standard_text() {
    assert_eq!(
        local_time::decode_base64("aGVsbG8="),
        Some(b"hello".to_vec())
    );
    assert_eq!(
        local_time::decode_base64("aGVs\nbG8="),
        Some(b"hello".to_vec())
    );
    assert_eq!(local_time::decode_base64("a$b"), None);
}

#[test]
fn byte_sizes_and_mime_types_are_readable() {
    assert_eq!(format_bytes(10), "10 B");
    assert_eq!(format_bytes(2048), "2.0 KB");
    assert_eq!(mime_for("Report.PDF"), "application/pdf");
    assert_eq!(mime_for("unknown.bin"), "application/octet-stream");
}

#[test]
fn an_attachment_only_message_still_lists_its_attachments() {
    let mut e = mail("a9", "t9", "Files", true);
    e.attachments = vec![Attachment {
        id: "att9".to_owned(),
        email_id: "a9".to_owned(),
        filename: "plan.pdf".to_owned(),
        mimetype: "application/pdf".to_owned(),
        size: 2048,
        content_id: None,
        disposition: None,
    }];
    let view = message_view(&e, &[]);
    assert_eq!(view.attachments[0].filename, "plan.pdf");
    assert_eq!(view.attachments[0].size, "2.0 KB");
}

#[test]
fn body_html_turns_markup_into_paragraphs_lists_and_links() {
    let text = "Hello **bold** and *it* [site](https://x.test/?a=1&b=2)\n\n- one\n- two\n\nNext";
    assert_eq!(
        body_html(text),
        "<p>Hello <strong>bold</strong> and <em>it</em> <a href=\"https://x.test/?a=1&amp;b=2\">site</a></p>\
<ul><li>one</li><li>two</li></ul><p>Next</p>"
    );
}

#[test]
fn compose_text_is_escaped_so_typed_markup_stays_text() {
    assert_eq!(
        body_html("<b>no</b> & yes"),
        "<p>&lt;b&gt;no&lt;/b&gt; &amp; yes</p>"
    );
}

#[test]
fn strip_elements_removes_scripts_in_any_case() {
    assert_eq!(
        strip_elements("<p>a</p><SCRIPT type=x>alert(1)</SCRIPT><p>b</p>", "script"),
        "<p>a</p><p>b</p>"
    );
}

#[test]
fn write_original_names_the_file_after_the_id_only() {
    let dir = test_dir().join("names");
    let mut e = mail("../../evil", "t", "x", true);
    let path = write_original(&dir, &e).unwrap();
    assert_eq!(path.file_name().unwrap(), "evil.html");
    e.id = "".to_owned();
    let path = write_original(&dir, &e).unwrap();
    assert_eq!(path.file_name().unwrap(), "message.html");
}
