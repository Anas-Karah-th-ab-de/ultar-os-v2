//! Einstellungen → Sicherheit: Geräteverschlüsselung (LUKS2) + verifizierter Start (dm-verity).
//! Backend: crate `ultar-secure` (orchestriert cryptsetup/veritysetup; keine eigene Kryptografie).
use super::App;
use crate::gfx::Rect;
use crate::ui::{Al, Btn, Ui};
use ultar_secure::{udata, verity};

impl App {
    pub(super) fn s_security(&mut self, ui: &mut Ui, area: Rect, sc: &mut f32) {
        let area = self.header(ui, area, self.t("Security"));
        ui.scroll_begin(area, sc);
        let mut y = area.y - *sc;
        let m = ui.dp(22.0);

        // --- Status ---
        ui.section(&mut y, self.t("Device encryption"));
        let enc = udata::is_encrypted();
        let status = if enc {
            self.t("Encrypted (AES-XTS, Argon2id)")
        } else {
            self.t("Not encrypted")
        };
        self.caption(ui, &mut y, status);

        if enc {
            // Passwort ändern
            ui.label(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(20.0)),
                self.t("Change password"),
                13.0,
                ui.th.sub,
                false,
                Al::Start,
            );
            y += ui.dp(26.0);
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "sec_old",
                &self.f("sec_old"),
                self.t("Current password"),
            );
            y += ui.dp(56.0);
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "sec_new",
                &self.f("sec_new"),
                self.t("New password"),
            );
            y += ui.dp(56.0);
            if ui.button(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                self.t("Change password"),
                Btn::Primary,
            ) {
                let (o, n) = (self.f("sec_old"), self.f("sec_new"));
                match udata::change_password(&o, &n) {
                    Ok(()) => self.toast(self.t("Done").into()),
                    Err(e) => self.toast(e),
                }
                self.fields.insert("sec_old", String::new());
                self.fields.insert("sec_new", String::new());
                self.focus = None;
            }
            y += ui.dp(58.0);
        } else {
            // Einschalten – WARNUNG: löscht /data
            let c = ui.th.danger;
            ui.para(
                m,
                y,
                ui.w - 2.0 * m,
                self.t("Turning on encryption erases all data on the device. Back up first."),
                13.0,
                c,
            );
            y += ui.dp(50.0);
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "sec_pw",
                &self.f("sec_pw"),
                self.t("New password"),
            );
            y += ui.dp(56.0);
            ui.field(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                "sec_pw2",
                &self.f("sec_pw2"),
                self.t("Repeat password"),
            );
            y += ui.dp(56.0);
            if ui.button(
                Rect::new(m, y, ui.w - 2.0 * m, ui.dp(48.0)),
                self.t("Encrypt device (erases data)"),
                Btn::Danger,
            ) {
                let (p1, p2) = (self.f("sec_pw"), self.f("sec_pw2"));
                if p1.len() < 6 {
                    self.toast(self.t("Password too short (min 6)").into());
                } else if p1 != p2 {
                    self.toast(self.t("Passwords do not match").into());
                } else {
                    match udata::enable(&p1) {
                        Ok(()) => self.toast(self.t("Encryption on — reboot to apply").into()),
                        Err(e) => self.toast(e),
                    }
                    self.fields.insert("sec_pw", String::new());
                    self.fields.insert("sec_pw2", String::new());
                    self.focus = None;
                }
            }
            y += ui.dp(58.0);
        }

        // --- Verified Boot ---
        ui.section(&mut y, self.t("Verified boot"));
        match verity::status("ultar-root") {
            Some(_) => self.caption(
                ui,
                &mut y,
                self.t("Active — system is hash-verified at every start"),
            ),
            None => self.caption(ui, &mut y, self.t("Inactive (not provisioned)")),
        }
        self.caption(
            ui,
            &mut y,
            self.t("The system part is read-only and cannot be changed unnoticed."),
        );

        ui.scroll_end(area, (y + *sc) - area.y + ui.dp(20.0), sc);
    }
}
