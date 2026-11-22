//! Echtes MTProto-Backend über grammers (nur mit Feature `real`).
//! Läuft in einem eigenen Thread mit Tokio-Laufzeit; die Oberfläche spricht nur das Trait.

use crate::{ChatSummary, LoginStage, Msg, TgBackend};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

enum Cmd {
    Connect {
        api_id: i32,
        api_hash: String,
        session: String,
    },
    RequestCode(String),
    SubmitCode(String),
    SubmitPassword(String),
    Open(i64),
    Send(i64, String),
}

#[derive(Default)]
struct Shared {
    stage: Option<LoginStage>,
    chats: Vec<ChatSummary>,
    msgs: Vec<Msg>,
    dirty: bool,
}

pub struct GrammersTg {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Shared>>,
    local_stage: LoginStage,
}

impl Default for GrammersTg {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let shared = Arc::new(Mutex::new(Shared {
            stage: Some(LoginStage::NeedPhone),
            dirty: true,
            ..Default::default()
        }));
        let sh2 = shared.clone();
        std::thread::spawn(move || worker(rx, sh2));
        Self {
            tx,
            shared,
            local_stage: LoginStage::NeedPhone,
        }
    }
}

impl TgBackend for GrammersTg {
    fn connect(&mut self, api_id: i32, api_hash: &str, session_path: &str) {
        let _ = self.tx.send(Cmd::Connect {
            api_id,
            api_hash: api_hash.into(),
            session: session_path.into(),
        });
    }
    fn request_code(&mut self, phone: &str) {
        let _ = self.tx.send(Cmd::RequestCode(phone.into()));
    }
    fn submit_code(&mut self, code: &str) {
        let _ = self.tx.send(Cmd::SubmitCode(code.into()));
    }
    fn submit_password(&mut self, password: &str) {
        let _ = self.tx.send(Cmd::SubmitPassword(password.into()));
    }
    fn stage(&self) -> LoginStage {
        self.local_stage.clone()
    }
    fn chats(&self) -> Vec<ChatSummary> {
        self.shared
            .lock()
            .map(|s| s.chats.clone())
            .unwrap_or_default()
    }
    fn open(&mut self, chat_id: i64) {
        let _ = self.tx.send(Cmd::Open(chat_id));
    }
    fn messages(&self) -> Vec<Msg> {
        self.shared
            .lock()
            .map(|s| s.msgs.clone())
            .unwrap_or_default()
    }
    fn send(&mut self, chat_id: i64, text: &str) {
        let _ = self.tx.send(Cmd::Send(chat_id, text.into()));
    }
    fn pump(&mut self) {
        if let Ok(mut s) = self.shared.lock() {
            if let Some(st) = s.stage.take() {
                self.local_stage = st;
            }
        }
    }
    fn dirty(&mut self) -> bool {
        self.shared
            .lock()
            .map(|mut s| std::mem::replace(&mut s.dirty, false))
            .unwrap_or(false)
    }
}

fn set_stage(sh: &Arc<Mutex<Shared>>, st: LoginStage) {
    if let Ok(mut s) = sh.lock() {
        s.stage = Some(st);
        s.dirty = true;
    }
}

fn worker(rx: Receiver<Cmd>, sh: Arc<Mutex<Shared>>) {
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            set_stage(&sh, LoginStage::Error(format!("Laufzeit: {e}")));
            return;
        }
    };
    rt.block_on(async move {
        use grammers_client::{Client, Config, SignInError};
        use grammers_session::Session;

        let mut client: Option<Client> = None;
        let mut login_token: Option<grammers_client::types::LoginToken> = None;
        let mut session_path = String::new();

        loop {
            // Befehle nicht-blockierend abholen; nebenbei Updates pumpen.
            match rx.try_recv() {
                Ok(cmd) => match cmd {
                    Cmd::Connect {
                        api_id,
                        api_hash,
                        session,
                    } => {
                        session_path = session.clone();
                        set_stage(&sh, LoginStage::Busy);
                        let sess = Session::load_file_or_create(&session)
                            .unwrap_or_else(|_| Session::new());
                        match Client::connect(Config {
                            session: sess,
                            api_id,
                            api_hash,
                            params: Default::default(),
                        })
                        .await
                        {
                            Ok(c) => {
                                let authed = c.is_authorized().await.unwrap_or(false);
                                client = Some(c);
                                if authed {
                                    set_stage(&sh, LoginStage::Ready);
                                    load_dialogs(client.as_ref().unwrap(), &sh).await;
                                } else {
                                    set_stage(&sh, LoginStage::NeedPhone);
                                }
                            }
                            Err(e) => set_stage(&sh, LoginStage::Error(format!("Verbindung: {e}"))),
                        }
                    }
                    Cmd::RequestCode(phone) => {
                        if let Some(c) = &client {
                            set_stage(&sh, LoginStage::Busy);
                            match c.request_login_code(&phone).await {
                                Ok(tok) => {
                                    login_token = Some(tok);
                                    set_stage(&sh, LoginStage::NeedCode);
                                }
                                Err(e) => set_stage(
                                    &sh,
                                    LoginStage::Error(format!("Code anfordern: {e}")),
                                ),
                            }
                        }
                    }
                    Cmd::SubmitCode(code) => {
                        if let (Some(c), Some(tok)) = (&client, &login_token) {
                            set_stage(&sh, LoginStage::Busy);
                            match c.sign_in(tok, &code).await {
                                Ok(_user) => {
                                    let _ = c.session().save_to_file(&session_path);
                                    set_stage(&sh, LoginStage::Ready);
                                    load_dialogs(c, &sh).await;
                                }
                                Err(SignInError::PasswordRequired(_pw)) => {
                                    set_stage(&sh, LoginStage::NeedPassword)
                                }
                                Err(e) => {
                                    set_stage(&sh, LoginStage::Error(format!("Anmeldung: {e}")))
                                }
                            }
                        }
                    }
                    Cmd::SubmitPassword(_password) => {
                        // check_password-API je nach grammers-Version; hier als offen markiert.
                        set_stage(
                            &sh,
                            LoginStage::Error("2FA-Passwort noch nicht implementiert".into()),
                        );
                    }
                    Cmd::Open(id) => {
                        if let Some(c) = &client {
                            load_history(c, id, &sh).await;
                        }
                    }
                    Cmd::Send(id, text) => {
                        if let Some(c) = &client {
                            if let Ok(chat) = resolve_chat(c, id).await {
                                let _ = c.send_message(&chat, text.as_str()).await;
                                load_history(c, id, &sh).await;
                            }
                        }
                    }
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // kurz auf Updates warten
                    if let Some(c) = &client {
                        if let Ok(Ok(_upd)) = tokio::time::timeout(
                            std::time::Duration::from_millis(400),
                            c.next_update(),
                        )
                        .await
                        {
                            if let Ok(mut s) = sh.lock() {
                                s.dirty = true;
                            }
                        }
                    } else {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
    });
}

async fn load_dialogs(c: &grammers_client::Client, sh: &Arc<Mutex<Shared>>) {
    let mut out = Vec::new();
    let mut dialogs = c.iter_dialogs();
    let mut n = 0;
    while let Ok(Some(d)) = dialogs.next().await {
        let chat = d.chat();
        let last = d
            .last_message
            .as_ref()
            .map(|m| m.text().to_string())
            .unwrap_or_default();
        out.push(ChatSummary {
            id: chat.id(),
            title: chat.name().to_string(),
            last,
            unread: 0,
        });
        n += 1;
        if n >= 60 {
            break;
        }
    }
    if let Ok(mut s) = sh.lock() {
        s.chats = out;
        s.dirty = true;
    }
}

async fn resolve_chat(
    c: &grammers_client::Client,
    id: i64,
) -> Result<grammers_client::types::Chat, ()> {
    let mut dialogs = c.iter_dialogs();
    while let Ok(Some(d)) = dialogs.next().await {
        let chat = d.chat();
        if chat.id() == id {
            return Ok(chat.clone());
        }
    }
    Err(())
}

async fn load_history(c: &grammers_client::Client, id: i64, sh: &Arc<Mutex<Shared>>) {
    let chat = match resolve_chat(c, id).await {
        Ok(ch) => ch,
        Err(_) => return,
    };
    let mut out = Vec::new();
    let mut it = c.iter_messages(&chat).limit(40);
    while let Ok(Some(m)) = it.next().await {
        out.push(Msg {
            out: m.outgoing(),
            text: m.text().to_string(),
            when: String::new(),
            sender: String::new(),
        });
    }
    out.reverse();
    if let Ok(mut s) = sh.lock() {
        s.msgs = out;
        s.dirty = true;
    }
}
