//! Game servers for owners and admins (port of the Expo `manage-servers.tsx`).
//!
//! Server types, packwiz packs and card groups come from the website, so new ones need no app
//! update. "File" servers (from `game-servers/*.json`) only take player-facing overrides;
//! servers added by staff are fully editable and can be deleted.

use std::rc::Rc;

use fwg_core::models::{
    AdminServer, AdminServerInput, AdminServerOptions, AdminServerTypeOption, GameDownloadPatch,
    GameServerType, NumOrText, ServerCallout, ServerSource, TextOrList,
};
use slint::{Color, ComponentHandle, ModelRc, SharedString, VecModel};

use super::List;
use crate::app::{App, error_chain};
use crate::{AppWindow, Choice, LoadState, Page, ServerForm, ServerRow, ServerState, platform};

const DEFAULT_ACCENT: &str = "#4f8fe8";
const DEFAULT_ORDER: &str = "1000";

#[derive(Default)]
pub(super) struct ServersData {
    list: std::cell::RefCell<List<AdminServer>>,
    options: std::cell::RefCell<AdminServerOptions>,
    /// The server being edited; `None` while adding one.
    editing: std::cell::RefCell<Option<String>>,
}

impl App {
    pub(super) fn bind_servers(self: &Rc<Self>, ui: &AppWindow) {
        let state = ui.global::<ServerState>();
        let weak = Rc::downgrade(self);
        let with = move |f: fn(&Rc<App>)| {
            let weak = weak.clone();
            move || {
                if let Some(app) = weak.upgrade() {
                    f(&app);
                }
            }
        };
        state.on_reload(with(|app| app.load_servers()));
        state.on_create(with(|app| app.create_server()));
        state.on_save(with(|app| app.save_server()));
        state.on_delete(with(|app| app.delete_server()));
        state.on_reset(with(|app| app.reset_server()));
        let weak = Rc::downgrade(self);
        state.on_open(move |slug| {
            if let Some(app) = weak.upgrade() {
                app.edit_server(&slug);
            }
        });
        state.on_accent_color(|text| parse_hex(&text).unwrap_or(Color::from_argb_u8(0, 0, 0, 0)));
    }

    fn server_ui(&self, f: impl FnOnce(ServerState<'_>)) {
        if let Some(ui) = self.ui.upgrade() {
            f(ui.global::<ServerState>());
        }
    }

    pub(super) fn clear_servers(&self) {
        *self.staff.servers.list.borrow_mut() = List::default();
        *self.staff.servers.editing.borrow_mut() = None;
        self.server_ui(|s| s.set_items(ModelRc::default()));
    }

    pub(super) fn load_servers(self: &Rc<Self>) {
        if !self.is_admin() {
            return;
        }
        {
            let mut list = self.staff.servers.list.borrow_mut();
            if list.loading {
                return;
            }
            list.loading = true;
        }
        self.render_servers();
        let Some(token) = self.session.token() else {
            return self.after_staff_request();
        };
        let api = self.api.clone();
        let task = self
            .rt
            .spawn(async move { api.admin_servers(&token).await });
        let app = self.clone();
        let _ = slint::spawn_local(async move {
            let result = task.await;
            {
                let mut list = app.staff.servers.list.borrow_mut();
                list.loading = false;
                match result {
                    Ok(Ok(response)) => {
                        list.items = response.items;
                        list.error = None;
                        *app.staff.servers.options.borrow_mut() =
                            response.options.unwrap_or_default();
                    }
                    Ok(Err(e)) => {
                        platform::log(&format!("servers: {}", error_chain(&e)));
                        list.error = Some(e.to_string());
                    }
                    Err(_) => list.error = Some("Servers could not be loaded.".into()),
                }
            }
            app.render_servers();
            app.after_staff_request();
        });
    }

    fn is_admin(&self) -> bool {
        self.session
            .user()
            .is_some_and(|u| u.role == "owner" || u.role == "admin")
    }

    fn render_servers(&self) {
        let list = self.staff.servers.list.borrow();
        let rows: Vec<ServerRow> = list.items.iter().map(server_row).collect();
        let state: LoadState = list.load_state();
        let can_create = {
            let o = self.staff.servers.options.borrow();
            o.type_options.is_some() || o.types.is_some()
        };
        self.server_ui(|s| {
            s.set_state(state);
            s.set_items(ModelRc::new(VecModel::from(rows)));
            s.set_can_create(can_create);
        });
    }

    fn create_server(self: &Rc<Self>) {
        *self.staff.servers.editing.borrow_mut() = None;
        self.fill_server_form(None, empty_form());
        self.navigate(Page::EditServer);
    }

    fn edit_server(self: &Rc<Self>, slug: &str) {
        let Some(server) = self
            .staff
            .servers
            .list
            .borrow()
            .items
            .iter()
            .find(|s| s.slug == slug)
            .cloned()
        else {
            return;
        };
        *self.staff.servers.editing.borrow_mut() = Some(server.slug.clone());
        self.fill_server_form(Some(&server), to_form(&server));
        self.navigate(Page::EditServer);
    }

    fn fill_server_form(&self, server: Option<&AdminServer>, form: ServerForm) {
        let options = self.staff.servers.options.borrow();
        let types = type_options(&options);
        let creating = server.is_none();
        let full = server.is_none_or(is_full);
        let type_info = types.iter().find(|t| t.value == form.server_type.as_str());
        let port = opt(type_info.and_then(|t| t.port).filter(|_| creating));
        let query = opt(type_info.and_then(|t| t.query_port).filter(|_| creating));
        let groups: Vec<Choice> = std::iter::once(choice("", "Its own card"))
            .chain(
                options
                    .groups
                    .iter()
                    .flatten()
                    .filter(|(key, _)| server.is_none_or(|s| &s.slug != *key))
                    .map(|(key, title)| choice(key, &format!("With {title}"))),
            )
            .collect();
        let packs: Vec<Choice> = std::iter::once(choice("", "None"))
            .chain(options.packs.iter().flatten().map(|p| choice(p, p)))
            .collect();
        // New servers show the type's usual ports as placeholders.
        let type_choices: Vec<Choice> = types
            .iter()
            .map(|t| Choice {
                port: opt(t.port.filter(|_| creating)),
                query: opt(t.query_port.filter(|_| creating)),
                ..choice(&t.value, &t.label)
            })
            .collect();
        let staff_added = server.is_some_and(|s| s.source == Some(ServerSource::Staff));
        let title = server.map_or_else(|| "Add server".to_owned(), title_of);
        let source_hint = server
            .filter(|s| !is_full(s))
            .map(|s| {
                format!(
                    "Name, colours, grouping, and modpack come from this server's file on the \
                     website (game-servers/{}.json). The fields below override it.",
                    s.slug
                )
            })
            .unwrap_or_default();
        let can_reset = server.is_some_and(|s| !staff_added && s.edited);
        self.server_ui(|s| {
            s.set_form(form);
            s.set_title(title.into());
            s.set_creating(creating);
            s.set_full(full);
            s.set_can_delete(staff_added);
            s.set_can_reset(can_reset);
            s.set_source_hint(source_hint.into());
            s.set_port_hint(port);
            s.set_query_hint(query);
            s.set_types(ModelRc::new(VecModel::from(type_choices)));
            s.set_groups(ModelRc::new(VecModel::from(groups)));
            s.set_packs(ModelRc::new(VecModel::from(packs)));
        });
    }

    fn save_server(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let form = ui.global::<ServerState>().get_form();
        let editing = self.staff.servers.editing.borrow().clone();
        let server = editing.as_ref().and_then(|slug| {
            self.staff
                .servers
                .list
                .borrow()
                .items
                .iter()
                .find(|s| &s.slug == slug)
                .cloned()
        });
        match server {
            None => {
                let values = to_input(&form, true, true);
                let slug = form.slug.trim().to_owned();
                self.staff_request(
                    move |api, token| async move { api.create_server(&token, &slug, &values).await },
                    |app, created| {
                        app.toast(&format!(
                            "Server created at /servers/{}. Live status appears within a minute.",
                            created.slug
                        ));
                        app.server_saved();
                    },
                );
            }
            Some(server) => {
                let values = to_input(&form, is_full(&server), false);
                let message = format!(
                    "{} saved.{}",
                    title_of(&server),
                    if form.active {
                        " Live status refreshes within a minute."
                    } else {
                        " It is now hidden from the website and app."
                    }
                );
                let slug = server.slug;
                self.staff_request(
                    move |api, token| async move { api.update_server(&token, &slug, &values).await },
                    move |app, _| {
                        app.toast(&message);
                        app.server_saved();
                    },
                );
            }
        }
    }

    /// Back to the list (which reloads it) after a save, delete or reset.
    fn server_saved(self: &Rc<Self>) {
        *self.staff.servers.editing.borrow_mut() = None;
        if self.page() == Page::EditServer {
            self.back();
        } else {
            self.load_servers();
        }
    }

    fn editing_server(&self) -> Option<AdminServer> {
        let slug = self.staff.servers.editing.borrow().clone()?;
        self.staff
            .servers
            .list
            .borrow()
            .items
            .iter()
            .find(|s| s.slug == slug)
            .cloned()
    }

    fn delete_server(self: &Rc<Self>) {
        let Some(server) = self.editing_server() else {
            return;
        };
        let title = title_of(&server);
        self.confirm(
            "Delete server?",
            &format!(
                "{title} will be removed from the website and app, and its page will stop \
                 working. This cannot be undone."
            ),
            "Delete",
            true,
            move |app| {
                let slug = server.slug;
                app.staff_request(
                    move |api, token| async move { api.delete_server(&token, &slug).await },
                    move |app, _| {
                        app.toast(&format!("{title} deleted."));
                        app.server_saved();
                    },
                );
            },
        );
    }

    fn reset_server(self: &Rc<Self>) {
        let Some(server) = self.editing_server() else {
            return;
        };
        let title = title_of(&server);
        self.confirm(
            "Reset to defaults?",
            &format!("Staff edits for {title} will be discarded and its server file used again."),
            "Reset",
            true,
            move |app| {
                let slug = server.slug;
                app.staff_request(
                    move |api, token| async move { api.reset_server(&token, &slug).await },
                    move |app, _| {
                        app.toast(&format!("{title} reset to defaults."));
                        app.server_saved();
                    },
                );
            },
        );
    }
}

fn choice(value: &str, label: &str) -> Choice {
    Choice {
        value: value.into(),
        label: label.into(),
        ..Default::default()
    }
}

fn title_of(server: &AdminServer) -> String {
    match server.variant.as_deref().filter(|v| !v.is_empty()) {
        Some(variant) => format!("{} — {variant}", server.name),
        None => server.name.clone(),
    }
}

/// Staff-added servers that came with their details are fully editable.
fn is_full(server: &AdminServer) -> bool {
    server.source == Some(ServerSource::Staff) && server.details.is_some()
}

fn type_name(kind: GameServerType) -> &'static str {
    match kind {
        GameServerType::A2s => "a2s",
        GameServerType::Minecraft => "minecraft",
        GameServerType::Fivem => "fivem",
        GameServerType::Hytale => "hytale",
        GameServerType::None => "none",
        GameServerType::Other => "other",
    }
}

/// The website's type list, or the built-in one for older website versions.
fn type_options(options: &AdminServerOptions) -> Vec<AdminServerTypeOption> {
    if let Some(types) = options.type_options.as_ref().filter(|t| !t.is_empty()) {
        return types.clone();
    }
    let fallback = [
        ("a2s", "Steam / Source query", Some(27015), Some(27016)),
        ("minecraft", "Minecraft Java", Some(25565), Some(25565)),
        ("fivem", "FiveM", Some(30120), Some(30120)),
        ("hytale", "Hytale", Some(5520), None),
        ("none", "No server: community group", None, None),
    ];
    fallback
        .into_iter()
        .filter(|(value, ..)| {
            options
                .types
                .as_ref()
                .is_none_or(|t| t.iter().any(|x| x == value))
        })
        .map(|(value, label, port, query_port)| AdminServerTypeOption {
            value: value.into(),
            label: label.into(),
            port,
            query_port,
        })
        .collect()
}

fn server_row(server: &AdminServer) -> ServerRow {
    let v = &server.values;
    let staff = server.source == Some(ServerSource::Staff);
    let address = if server.kind == GameServerType::None {
        "Community group".to_owned()
    } else if !v.connect.is_empty() {
        v.connect.clone()
    } else if !v.host.is_empty() {
        match v.port {
            Some(port) => format!("{}:{port}", v.host),
            None => v.host.clone(),
        }
    } else {
        "No address set".to_owned()
    };
    let mut meta = Vec::new();
    if let Some(operator) = server
        .details
        .as_ref()
        .map(|d| &d.operator)
        .filter(|o| !o.is_empty())
    {
        meta.push(format!("Run by {operator}"));
    }
    if let Some(by) = server.updated_by.as_ref().filter(|_| server.edited) {
        meta.push(format!("Last edited by {by}"));
    }
    ServerRow {
        slug: server.slug.as_str().into(),
        title: title_of(server).into(),
        badge: if staff {
            "ADDED BY STAFF"
        } else if server.edited {
            "EDITED"
        } else {
            "DEFAULTS"
        }
        .into(),
        edited: server.edited,
        inactive: !v.active,
        address: address.into(),
        meta: meta.join(" · ").into(),
    }
}

fn empty_form() -> ServerForm {
    ServerForm {
        active: true,
        server_type: "minecraft".into(),
        accent: DEFAULT_ACCENT.into(),
        order: DEFAULT_ORDER.into(),
        ..Default::default()
    }
}

fn opt<T: ToString>(value: Option<T>) -> SharedString {
    value.map(|v| v.to_string()).unwrap_or_default().into()
}

fn to_form(server: &AdminServer) -> ServerForm {
    let v = &server.values;
    let mut form = ServerForm {
        active: v.active,
        server_type: type_name(server.kind).into(),
        description: v.description.as_str().into(),
        host: v.host.as_str().into(),
        port: opt(v.port),
        query_port: opt(v.query_port),
        connect: v.connect.as_str().into(),
        join: v.join.join("\n").into(),
        rules: v.rules.join("\n").into(),
        notes: v.notes.as_str().into(),
        mods_url: v.mods_url.as_str().into(),
        discord_channel: v.discord_channel.as_str().into(),
        ..empty_form()
    };
    if let Some(d) = &server.details {
        let dl = &d.download;
        form.name = d.name.as_str().into();
        form.variant = d.variant.as_str().into();
        form.game = d.game.as_str().into();
        form.operator = d.operator.as_str().into();
        form.genre = d.genre.as_str().into();
        form.accent = d.accent.as_str().into();
        form.url = d.url.as_str().into();
        form.image = d.image.as_str().into();
        form.order = d.order.to_string().into();
        form.modpack = d.modpack.as_str().into();
        form.modpack_manual = d.modpack_manual.join("\n").into();
        form.dl_url = opt(dl.url.as_ref());
        form.dl_name = opt(dl.name.as_ref());
        form.dl_version = opt(dl.version.as_ref());
        form.dl_minecraft = opt(dl.minecraft.as_ref());
        form.dl_loader = opt(dl.loader.as_ref());
        form.dl_mods = match &dl.mods {
            Some(NumOrText::Num(n)) => n.to_string().into(),
            Some(NumOrText::Text(t)) => t.as_str().into(),
            None => SharedString::new(),
        };
        form.callout_title = opt(d.callout.as_ref().and_then(|c| c.title.as_ref()));
        form.callout_text = opt(d.callout.as_ref().and_then(|c| c.text.as_ref()));
    }
    form
}

/// The request body, as the Expo app builds it: only file-server overrides unless `full`.
fn to_input(form: &ServerForm, full: bool, creating: bool) -> AdminServerInput {
    let text = |s: &SharedString| Some(s.to_string());
    let trimmed = |s: &SharedString| Some(s.trim().to_owned());
    let number = |s: &SharedString| {
        let s = s.trim();
        (!s.is_empty()).then(|| NumOrText::Text(s.to_owned()))
    };
    let mut out = AdminServerInput {
        active: Some(form.active),
        description: text(&form.description),
        join: Some(TextOrList::Text(form.join.to_string())),
        rules: Some(TextOrList::Text(form.rules.to_string())),
        notes: text(&form.notes),
        mods_url: trimmed(&form.mods_url),
        discord_channel: trimmed(&form.discord_channel),
        ..Default::default()
    };
    if form.server_type != "none" {
        out.host = trimmed(&form.host);
        out.connect = trimmed(&form.connect);
        out.port = number(&form.port);
        out.query_port = number(&form.query_port);
    }
    if full {
        out.name = text(&form.name);
        out.variant = text(&form.variant);
        out.game = text(&form.game);
        out.operator = text(&form.operator);
        out.genre = text(&form.genre);
        out.accent = trimmed(&form.accent);
        out.url = trimmed(&form.url);
        out.image = trimmed(&form.image);
        out.order = Some(NumOrText::Text(match form.order.trim() {
            "" => DEFAULT_ORDER.to_owned(),
            o => o.to_owned(),
        }));
        out.callout = Some(ServerCallout {
            title: text(&form.callout_title),
            text: text(&form.callout_text),
        });
        if creating {
            out.kind = text(&form.server_type);
        }
        if form.server_type == "minecraft" {
            out.modpack = text(&form.modpack);
            out.modpack_manual = Some(form.modpack_manual.split('\n').map(str::to_owned).collect());
            out.download = Some(if form.modpack.is_empty() {
                GameDownloadPatch {
                    url: trimmed(&form.dl_url),
                    name: text(&form.dl_name),
                    version: text(&form.dl_version),
                    minecraft: text(&form.dl_minecraft),
                    loader: text(&form.dl_loader),
                    mods: Some(NumOrText::Text(form.dl_mods.trim().to_owned())),
                }
            } else {
                GameDownloadPatch {
                    url: Some(String::new()),
                    ..Default::default()
                }
            });
        }
    }
    out
}

/// "#4f8fe8" → colour, for the accent swatch.
fn parse_hex(text: &str) -> Option<Color> {
    let hex = text.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some(Color::from_rgb_u8(
        (value >> 16) as u8,
        (value >> 8) as u8,
        value as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_accent() {
        assert_eq!(
            parse_hex("#4f8fe8"),
            Some(Color::from_rgb_u8(0x4f, 0x8f, 0xe8))
        );
        assert_eq!(parse_hex("4f8fe8"), None);
        assert_eq!(parse_hex("#4f8"), None);
    }

    #[test]
    fn file_server_input_has_only_overrides() {
        let form = ServerForm {
            host: " play.example.com ".into(),
            port: "".into(),
            ..empty_form()
        };
        let input = to_input(&form, false, false);
        assert_eq!(input.host.as_deref(), Some("play.example.com"));
        assert!(input.port.is_none());
        assert!(input.name.is_none() && input.kind.is_none() && input.download.is_none());
    }

    #[test]
    fn new_minecraft_server_sends_type_and_download() {
        let form = ServerForm {
            name: "Minecraft".into(),
            dl_url: "/packs/a.zip".into(),
            ..empty_form()
        };
        let input = to_input(&form, true, true);
        assert_eq!(input.kind.as_deref(), Some("minecraft"));
        assert_eq!(
            input.download.and_then(|d| d.url).as_deref(),
            Some("/packs/a.zip")
        );
        assert!(matches!(input.order, Some(NumOrText::Text(ref o)) if o == "1000"));
    }

    #[test]
    fn fallback_types_follow_website_list() {
        let options = AdminServerOptions {
            types: Some(vec!["minecraft".into(), "none".into()]),
            ..Default::default()
        };
        let values: Vec<_> = type_options(&options)
            .into_iter()
            .map(|t| t.value)
            .collect();
        assert_eq!(values, ["minecraft", "none"]);
    }
}
