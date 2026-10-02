//! Applications started behind a security context (doc_bar.md, BR2), and the shell's own
//! COSMIC components started on the main socket.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::io::{self, ErrorKind};
use std::os::fd::AsFd;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::time::Duration;

use gtk4::gio::{self, prelude::*};
use gtk4::glib::{self, Variant};

use crate::connection::{Client, Error};
use crate::unit::{self, Unit};

/// Runs a command inside the user's default terminal (freedesktop's terminal
/// intent specification).
const TERMINAL: &str = "xdg-terminal-exec";

#[derive(Debug, thiserror::Error)]
pub enum LaunchError {
    #[error("{app} cannot be started: {reason}")]
    Entry { app: String, reason: String },
    #[error("{0} is not installed")]
    Missing(String),
    #[error(transparent)]
    Compositor(#[from] Error),
    #[error("the socket of the application could not be prepared: {0}")]
    Socket(#[from] std::io::Error),
    #[error("the session bus refused the request: {0}")]
    Bus(#[from] glib::Error),
    #[error("{0} did not appear on the session bus")]
    NoAnswer(&'static str),
    #[error("{unit} did not start: its start job ended with \"{result}\"")]
    Start { unit: String, result: String },
    #[error("the file cannot be read: {0}")]
    File(glib::Error),
}

/// What the field codes of an `Exec` line expand to.
pub(crate) struct Fields<'a> {
    pub(crate) name: &'a str,
    pub(crate) icon: Option<&'a str>,
    pub(crate) location: Option<&'a str>,
    /// The one file or URL to open (doc_launcher.md, LA5); none when the application
    /// only starts.
    pub(crate) target: Option<Target<'a>>,
}

#[derive(Clone, Copy)]
pub(crate) struct Target<'a> {
    pub(crate) uri: &'a str,
    /// The local path of `uri`; `None` when it is remote.
    pub(crate) path: Option<&'a str>,
}

/// What a file or URL code becomes: the target as a path (`%f`, `%F`) or as a URI (`%u`,
/// `%U`), or nothing when there is no target.
fn target_for(code: char, fields: &Fields<'_>) -> Result<Option<String>, String> {
    let Some(target) = fields.target else { return Ok(None) };
    match code {
        'f' | 'F' => target
            .path
            .map(|path| Some(path.to_owned()))
            .ok_or_else(|| "the application opens local files only".to_owned()),
        _ => Ok(Some(target.uri.to_owned())),
    }
}

/// The arguments of an `Exec` line, with its field codes expanded (Desktop Entry
/// Specification, "The Exec key"), quoted as GLib's own launcher parses it. A target the
/// line has no code for is refused: the application would start without it. So is a target
/// that would stand alone as an argument starting with `-`: the application would read it
/// as an option.
pub(crate) fn expand(exec: &str, fields: &Fields<'_>) -> Result<Vec<String>, String> {
    let words = glib::shell_parse_argv(exec).map_err(|err| err.to_string())?;
    let mut argv = Vec::new();
    let mut used = false;
    for word in words {
        let word = word
            .into_string()
            .map_err(|_| "an argument is not UTF-8".to_owned())?;
        match word.as_str() {
            "%f" | "%F" | "%u" | "%U" => {
                let code = word.chars().nth(1).unwrap_or('u');
                if let Some(arg) = target_for(code, fields)? {
                    if arg.starts_with('-') {
                        return Err("the file name would be read as an option".to_owned());
                    }
                    argv.push(arg);
                    used = true;
                }
            }
            // Deprecated codes, removed.
            "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => {}
            "%i" => {
                if let Some(icon) = fields.icon {
                    argv.extend(["--icon".to_owned(), icon.to_owned()]);
                }
            }
            _ => argv.push(expand_word(&word, fields, &mut used)?),
        }
    }
    if argv.is_empty() {
        return Err("the Exec line names no program".to_owned());
    }
    if fields.target.is_some() && !used {
        return Err("the application does not take a file to open".to_owned());
    }
    Ok(argv)
}

fn expand_word(word: &str, fields: &Fields<'_>, used: &mut bool) -> Result<String, String> {
    let mut out = String::with_capacity(word.len());
    let mut chars = word.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('c') => out.push_str(fields.name),
            Some('k') => out.push_str(fields.location.unwrap_or_default()),
            Some(code @ ('f' | 'F' | 'u' | 'U')) => {
                if let Some(arg) = target_for(code, fields)? {
                    out.push_str(&arg);
                    *used = true;
                }
            }
            Some('d' | 'D' | 'n' | 'N' | 'v' | 'm') => {}
            Some(code) => return Err(format!("the field code %{code} is not valid here")),
            None => return Err("the Exec line ends with a lone %".to_owned()),
        }
    }
    Ok(out)
}

/// An absolute path for `program`, searched in the shell's `PATH`.
fn resolve(program: &str) -> Result<String, LaunchError> {
    glib::find_program_in_path(program)
        .and_then(|path| path.into_os_string().into_string().ok())
        .ok_or_else(|| LaunchError::Missing(program.to_owned()))
}

/// `<runtime>/athanor`, created private when missing. It must be a real directory, not a
/// symbolic link: the sockets and capture buffers of the shell's programs live in it.
pub(crate) fn runtime_subdir(runtime: &std::path::Path) -> io::Result<PathBuf> {
    let parent = runtime.join("athanor");
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&parent)?;
    if !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(io::Error::new(
            ErrorKind::NotADirectory,
            format!("{} is not a directory", parent.display()),
        ));
    }
    Ok(parent)
}

/// `<runtime>/athanor/<random>`, the directory of an application's socket. `athanor` is
/// created private when missing and must be a directory, not a symbolic link; the leaf must
/// not exist yet, so no one else prepared it. `runtime` is `$XDG_RUNTIME_DIR`, refused when
/// unset or not absolute: the home directory GLib falls back to is not private to a session.
fn socket_dir(runtime: Option<OsString>, random: &str) -> Result<PathBuf, LaunchError> {
    let runtime = runtime
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
        .ok_or_else(|| {
            io::Error::new(
                ErrorKind::NotFound,
                "XDG_RUNTIME_DIR is not set to an absolute path",
            )
        })?;
    let parent = runtime_subdir(&runtime)?;
    let dir = parent.join(random);
    DirBuilder::new().mode(0o700).create(&dir)?;
    Ok(dir)
}

/// `uri` in GIO's canonical form and its local path, or `None` when it has no scheme.
fn canonical(uri: &str) -> Option<(String, Option<PathBuf>)> {
    let file = gio::File::for_uri(uri);
    file.uri_scheme()?;
    Some((file.uri().to_string(), file.path()))
}

/// The desktop id the security context and the unit of a typed command carry.
const COMMAND_ID: &str = "os.athanor.Command.desktop";

/// What a launch starts, whether from a desktop entry or a typed command.
struct Start<'a> {
    id: &'a str,
    name: &'a str,
    /// For the activation token; none for a command.
    info: Option<&'a gio::AppInfo>,
    working_directory: String,
}

impl Client {
    /// Starts `app` in a transient service of the user manager, on a socket of its own
    /// behind a security context, and returns the unit's name. When the context cannot be
    /// created the application does not start (BR2.6).
    pub async fn launch(&self, app: &gio_unix::DesktopAppInfo) -> Result<String, LaunchError> {
        self.launch_with(app, None).await
    }

    /// As [`Client::launch`], handing `app` one file or URL as its `Exec` line takes it.
    pub async fn launch_with(
        &self,
        app: &gio_unix::DesktopAppInfo,
        uri: Option<&str>,
    ) -> Result<String, LaunchError> {
        self.live()?;
        let name = app.name();
        let entry = |reason: &str| LaunchError::Entry {
            app: name.to_string(),
            reason: reason.to_owned(),
        };
        let id = app.id().ok_or_else(|| entry("it has no desktop id"))?;
        // DBusActivatable is ignored on purpose: bus activation would start the
        // application with the user manager's environment, on the main socket (BR2.5).
        let exec = app
            .string("Exec")
            .ok_or_else(|| entry("it has no Exec line"))?;
        let icon = app.string("Icon");
        let location = app
            .filename()
            .and_then(|path| path.into_os_string().into_string().ok());
        // A target is a URI with a scheme, in GIO's canonical form: a bare string such as
        // `+cmd` is not one, and the application would read it as an option.
        let canonical = match uri {
            Some(uri) => Some(canonical(uri).ok_or_else(|| entry("the target is not a URI"))?),
            None => None,
        };
        let path = canonical
            .as_ref()
            .and_then(|(_, path)| path.clone())
            .and_then(|path| path.into_os_string().into_string().ok());
        let fields = Fields {
            name: &name,
            icon: icon.as_deref(),
            location: location.as_deref(),
            target: canonical.as_ref().map(|(uri, _)| Target {
                uri,
                path: path.as_deref(),
            }),
        };
        let mut argv = expand(&exec, &fields).map_err(|reason| entry(&reason))?;
        if app.boolean("Terminal") {
            argv.splice(0..0, [TERMINAL.to_owned(), "--".to_owned()]);
        }
        let working_directory = app
            .string("Path")
            .filter(|path| path.starts_with('/'))
            .map_or_else(|| "~".to_owned(), |path| path.to_string());
        let start = Start {
            id: &id,
            name: &name,
            info: Some(app.upcast_ref()),
            working_directory,
        };
        self.start(&start, argv).await
    }

    /// Opens a file or a URL with the user's default application for it, started like any
    /// other (BR2). A local file is typed by its content, as the file manager types it.
    pub async fn open_uri(&self, uri: &str) -> Result<String, LaunchError> {
        // Not a URI GIO can name, and an argument an application could read as an option.
        if uri.starts_with('-') {
            return Err(LaunchError::Entry {
                app: "the file or URL".to_owned(),
                reason: "its name would be read as an option".to_owned(),
            });
        }
        let file = gio::File::for_uri(uri);
        let app = if file.is_native() {
            let info = file
                .query_info_future(
                    gio::FILE_ATTRIBUTE_STANDARD_CONTENT_TYPE,
                    gio::FileQueryInfoFlags::NONE,
                    glib::Priority::DEFAULT,
                )
                .await
                .map_err(LaunchError::File)?;
            let kind = info
                .content_type()
                .ok_or_else(|| LaunchError::Missing(format!("a type for {uri}")))?;
            gio::AppInfo::default_for_type(&kind, false)
                .ok_or_else(|| LaunchError::Missing(format!("an application for {kind}")))?
        } else {
            // From the text: GIO's own answer for an https URL is "http" (gvfs' web backend
            // serves both), which would pick the handler of the wrong scheme.
            let scheme = glib::Uri::peek_scheme(uri).unwrap_or_default();
            gio::AppInfo::default_for_uri_scheme(&scheme)
                .ok_or_else(|| LaunchError::Missing(format!("an application for {scheme}:")))?
        };
        let app = app
            .downcast::<gio_unix::DesktopAppInfo>()
            .map_err(|app| LaunchError::Missing(format!("a desktop entry for {}", app.name())))?;
        self.launch_with(&app, Some(uri)).await
    }

    /// Runs a command line in the user's default terminal, in a unit and behind a
    /// security context of its own like an application (doc_launcher.md, LA5). `argv` is
    /// the user's own command, run exactly as given, after `--` so the terminal launcher
    /// takes none of it as an option; a first word starting with `-` is refused as well.
    pub async fn launch_command(&self, argv: &[String]) -> Result<String, LaunchError> {
        self.live()?;
        let command = |reason: &str| LaunchError::Entry {
            app: "the command".to_owned(),
            reason: reason.to_owned(),
        };
        let Some(name) = argv.first().filter(|name| !name.is_empty()) else {
            return Err(command("it is empty"));
        };
        if name.starts_with('-') {
            return Err(command("its program name would be read as an option"));
        }
        let start = Start {
            id: COMMAND_ID,
            name,
            info: None,
            working_directory: "~".to_owned(),
        };
        let argv = [vec![TERMINAL.to_owned(), "--".to_owned()], argv.to_vec()].concat();
        self.start(&start, argv).await
    }

    async fn start(&self, start: &Start<'_>, argv: Vec<String>) -> Result<String, LaunchError> {
        let entry = |reason: &str| LaunchError::Entry {
            app: start.name.to_owned(),
            reason: reason.to_owned(),
        };
        let (program, arguments) = argv
            .split_first()
            .ok_or_else(|| entry("the Exec line names no program"))?;
        let argv = [vec![resolve(program)?], arguments.to_vec()].concat();

        let random = unit::random();
        let unit_name = unit::app_unit_name(start.id, &random)
            .ok_or_else(|| entry("its desktop id is too long"))?;
        let runtime_directory = format!("athanor/{random}");
        let dir = socket_dir(std::env::var_os("XDG_RUNTIME_DIR"), &random)?;
        let result = self
            .start_in_context(start, argv, unit_name, runtime_directory, &dir)
            .await;
        if result.is_err() {
            // systemd removes it too when the unit it started stops.
            match fs::remove_dir_all(&dir) {
                Err(err) if err.kind() != ErrorKind::NotFound => {
                    tracing::warn!(dir = %dir.display(), "the socket directory was not removed: {err}");
                }
                _ => {}
            }
        }
        result
    }

    async fn start_in_context(
        &self,
        start: &Start<'_>,
        argv: Vec<String>,
        unit_name: String,
        runtime_directory: String,
        dir: &std::path::Path,
    ) -> Result<String, LaunchError> {
        let socket = dir.join("wayland");
        let listener = UnixListener::bind(&socket)?;
        let (close_read, close_write) = std::io::pipe()?;
        // The compositor keeps its own copies; ours close when this function returns. The
        // context lasts until every copy of the write end is closed: the user manager
        // holds one for as long as the unit runs.
        self.create_context(
            listener.as_fd(),
            close_read.as_fd(),
            unit::app_id(start.id),
            &unit_name,
        )?;

        let mut environment = vec![format!("WAYLAND_DISPLAY={}", socket.display())];
        if let Some(token) = self.activation_token(start.info) {
            environment.push(format!("XDG_ACTIVATION_TOKEN={token}"));
            environment.push(format!("DESKTOP_STARTUP_ID={token}"));
        }
        let unit = Unit {
            name: unit_name,
            description: start.name.to_owned(),
            argv,
            environment,
            working_directory: start.working_directory.clone(),
            runtime_directory: Some(runtime_directory),
        };
        unit.start(Some(close_write.into())).await?;
        Ok(unit.name)
    }
}

/// The COSMIC components the shell opens until its own replace them (doc_bar.md, BR3).
/// They are part of the shell and keep the main socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opener {
    Launcher,
    AppLibrary,
    Workspaces,
}

impl Opener {
    /// The well-known name of libcosmic's single instance: the app id.
    fn app_id(self) -> &'static str {
        match self {
            Opener::Launcher => "com.system76.CosmicLauncher",
            Opener::AppLibrary => "com.system76.CosmicAppLibrary",
            Opener::Workspaces => "com.system76.CosmicWorkspaces",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Opener::Launcher => "cosmic-launcher",
            Opener::AppLibrary => "cosmic-app-library",
            Opener::Workspaces => "cosmic-workspaces",
        }
    }

    /// The object libcosmic exports its activation interface on.
    fn path(self) -> String {
        format!("/{}", self.app_id().replace('.', "/"))
    }

    /// The interface, method and arguments of the call that shows the component and
    /// leaves it shown when it already is. In COSMIC 1.8 `Activate` toggles all three, and
    /// cosmic-launcher ignores it during its first 100 ms. The launcher and the application
    /// library take the `Input` action with no text instead, which only shows.
    /// cosmic-workspaces has a `Show` method, but exports it only after it takes its name;
    /// a process started cold is hidden, so `Activate` shows it.
    fn show_call(
        self,
        cold: bool,
        platform_data: HashMap<&str, Variant>,
    ) -> (&'static str, &'static str, Variant) {
        match self {
            Opener::Launcher | Opener::AppLibrary => (
                ACTIVATION,
                "ActivateAction",
                (SHOW_ACTION, Vec::<String>::new(), platform_data).to_variant(),
            ),
            Opener::Workspaces if cold => (ACTIVATION, "Activate", (platform_data,).to_variant()),
            Opener::Workspaces => (self.app_id(), "Show", ().to_variant()),
        }
    }
}

/// libcosmic's single-instance interface, exported beside the app id's name.
const ACTIVATION: &str = "org.freedesktop.DbusActivation";
/// A libcosmic action is the JSON form of the component's subcommand: here `Input` with no
/// text, which cosmic-launcher and cosmic-app-library both define.
const SHOW_ACTION: &str = r#"{"Input":{"input":null}}"#;

/// How long a component has to take its name after it was started.
const APPEAR: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(50);

impl Client {
    /// Shows the component, starting it first when it does not run, and leaves it shown
    /// when it already is. A component started cold only takes its name, so it is always
    /// shown through the bus (spike P4).
    pub async fn open(&self, opener: Opener) -> Result<(), LaunchError> {
        self.live()?;
        let bus = gio::bus_get_future(gio::BusType::Session).await?;
        let name = opener.app_id();
        let cold = !has_owner(&bus, name).await?;
        if cold {
            self.start_component(opener).await?;
            let mut waited = Duration::ZERO;
            while !has_owner(&bus, name).await? {
                if waited >= APPEAR {
                    return Err(LaunchError::NoAnswer(name));
                }
                glib::timeout_future(POLL).await;
                waited += POLL;
            }
        }
        let mut platform_data = HashMap::<&str, Variant>::new();
        if let Some(token) = self.activation_token(None) {
            platform_data.insert("activation-token", token.to_variant());
            platform_data.insert("desktop-startup-id", token.to_variant());
        }
        let (interface, method, parameters) = opener.show_call(cold, platform_data);
        bus.call_future(
            Some(name),
            &opener.path(),
            interface,
            method,
            Some(&parameters),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
        Ok(())
    }

    async fn start_component(&self, opener: Opener) -> Result<(), LaunchError> {
        // The socket this client is connected to, whatever the process's environment says.
        let environment = vec![format!("WAYLAND_DISPLAY={}", self.display_name()?)];
        let random = unit::random();
        let unit = Unit {
            name: unit::app_unit_name(opener.app_id(), &random).ok_or_else(|| {
                LaunchError::Entry {
                    app: opener.program().to_owned(),
                    reason: "its desktop id is too long".to_owned(),
                }
            })?,
            description: opener.program().to_owned(),
            argv: vec![resolve(opener.program())?],
            environment,
            working_directory: "~".to_owned(),
            runtime_directory: None,
        };
        unit.start(None).await?;
        Ok(())
    }
}

async fn has_owner(bus: &gio::DBusConnection, name: &str) -> Result<bool, glib::Error> {
    let reply = bus
        .call_future(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await?;
    Ok(reply.get::<(bool,)>().is_some_and(|(owned,)| owned))
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn an_https_url_is_looked_up_by_its_own_scheme() {
        assert_eq!(glib::Uri::peek_scheme("https://duckduckgo.com/?q=a").as_deref(), Some("https"));
        assert_eq!(glib::Uri::peek_scheme("HTTPS://example.org").as_deref(), Some("https"));
        assert_eq!(glib::Uri::peek_scheme("not a uri"), None);
    }

    const FIELDS: Fields<'static> = Fields {
        name: "Text Editor",
        icon: Some("org.gnome.TextEditor"),
        location: Some("/usr/share/applications/org.gnome.TextEditor.desktop"),
        target: None,
    };

    fn with(uri: &'static str, path: Option<&'static str>) -> Fields<'static> {
        Fields {
            target: Some(Target { uri, path }),
            ..FIELDS
        }
    }

    #[test]
    fn a_target_goes_where_the_exec_line_asks() {
        let file = with("file:///h/a%20b.png", Some("/h/a b.png"));
        assert_eq!(expand("viewer %f", &file).unwrap(), ["viewer", "/h/a b.png"]);
        assert_eq!(expand("viewer %F", &file).unwrap(), ["viewer", "/h/a b.png"]);
        assert_eq!(
            expand("browser %U", &file).unwrap(),
            ["browser", "file:///h/a%20b.png"]
        );
        assert_eq!(
            expand("app --open=%u", &file).unwrap(),
            ["app", "--open=file:///h/a%20b.png"]
        );
        let web = with("https://example.org/?q=a", None);
        assert_eq!(
            expand("firefox %u", &web).unwrap(),
            ["firefox", "https://example.org/?q=a"]
        );
    }

    #[test]
    fn a_remote_target_needs_a_url_code() {
        let web = with("https://example.org/", None);
        assert!(expand("viewer %f", &web).is_err());
    }

    #[test]
    fn a_target_with_nowhere_to_go_is_refused() {
        let file = with("file:///h/a.txt", Some("/h/a.txt"));
        assert!(
            expand("app", &file).is_err(),
            "the application would start without the file"
        );
        assert!(expand("app %i", &file).is_err());
    }

    #[test]
    fn a_target_never_becomes_two_arguments() {
        let file = with("file:///h/x;%20touch%20y", Some("/h/x; touch y"));
        assert_eq!(
            expand("viewer %f", &file).unwrap(),
            ["viewer", "/h/x; touch y"]
        );
    }

    #[test]
    fn a_target_without_a_scheme_is_refused() {
        assert!(canonical("+cmd").is_none());
        assert!(canonical("-rf").is_none());
        let (uri, path) = canonical("file:///h/a%20b").unwrap();
        assert_eq!(uri, "file:///h/a%20b");
        assert_eq!(path.unwrap(), PathBuf::from("/h/a b"));
    }

    #[test]
    fn a_target_is_never_an_option() {
        let file = with("-rf", Some("-rf"));
        assert!(expand("viewer %f", &file).is_err());
        assert!(expand("viewer %u", &file).is_err());
        // Inside a longer word it cannot be read as one.
        assert_eq!(expand("viewer --file=%f", &file).unwrap(), ["viewer", "--file=-rf"]);
    }

    fn run(exec: &str) -> Result<Vec<String>, String> {
        expand(exec, &FIELDS)
    }

    #[test]
    fn file_and_url_codes_disappear_and_the_rest_expand() {
        assert_eq!(run("gnome-text-editor %U").unwrap(), ["gnome-text-editor"]);
        assert_eq!(
            run("app %i --title=%c %k 100%% %f").unwrap(),
            [
                "app",
                "--icon",
                "org.gnome.TextEditor",
                "--title=Text Editor",
                "/usr/share/applications/org.gnome.TextEditor.desktop",
                "100%"
            ]
        );
        let no_icon = Fields {
            icon: None,
            ..FIELDS
        };
        assert_eq!(expand("app %i", &no_icon).unwrap(), ["app"]);
    }

    #[test]
    fn quoting_follows_the_specification() {
        assert_eq!(
            run(r#""/opt/My App/bin/app" --arg "a \"b\" c" --x=%u"#).unwrap(),
            ["/opt/My App/bin/app", "--arg", "a \"b\" c", "--x="]
        );
    }

    #[test]
    fn malformed_lines_are_refused() {
        for exec in [
            "",
            "   ",
            "%f",
            "app %z",
            "app 50%",
            "app \"unterminated",
            "app --icon=%i",
        ] {
            assert!(run(exec).is_err(), "{exec:?} was accepted");
        }
    }

    #[test]
    fn openers_address_libcosmic_single_instances() {
        assert_eq!(Opener::Launcher.path(), "/com/system76/CosmicLauncher");
        assert_eq!(Opener::AppLibrary.app_id(), "com.system76.CosmicAppLibrary");
        assert_eq!(Opener::Workspaces.program(), "cosmic-workspaces");
    }

    #[test]
    fn openers_show_without_toggling() {
        let data = || HashMap::from([("activation-token", "t".to_variant())]);
        for opener in [Opener::Launcher, Opener::AppLibrary] {
            for cold in [true, false] {
                let (interface, method, parameters) = opener.show_call(cold, data());
                assert_eq!((interface, method), (ACTIVATION, "ActivateAction"));
                assert_eq!(parameters.type_().as_str(), "(sasa{sv})");
                assert_eq!(parameters.child_value(0).str(), Some(SHOW_ACTION));
                assert_eq!(parameters.child_value(1).n_children(), 0);
                assert_eq!(parameters.child_value(2).n_children(), 1);
            }
        }
        let (interface, method, parameters) = Opener::Workspaces.show_call(true, data());
        assert_eq!((interface, method), (ACTIVATION, "Activate"));
        assert_eq!(parameters.type_().as_str(), "(a{sv})");
        let (interface, method, parameters) = Opener::Workspaces.show_call(false, data());
        assert_eq!(
            (interface, method),
            ("com.system76.CosmicWorkspaces", "Show")
        );
        assert_eq!(parameters.type_().as_str(), "()");
    }

    /// A directory of its own under the system's temporary directory, removed on drop.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("cc-{name}-{}", unit::random()));
            fs::create_dir(&dir).unwrap();
            Scratch(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn the_socket_directory_is_new_and_private() {
        let runtime = Scratch::new("runtime");
        let dir = socket_dir(Some(runtime.0.clone().into()), "leaf").unwrap();
        assert_eq!(dir, runtime.0.join("athanor/leaf"));
        for path in [&dir, &runtime.0.join("athanor")] {
            let mode = fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o700, "{}", path.display());
        }
        // A second preparation of the same leaf is refused: someone else holds it.
        let again = socket_dir(Some(runtime.0.clone().into()), "leaf").unwrap_err();
        assert!(
            matches!(&again, LaunchError::Socket(err) if err.kind() == ErrorKind::AlreadyExists),
            "{again}"
        );
    }

    #[test]
    fn a_symlinked_socket_parent_is_refused() {
        let runtime = Scratch::new("runtime");
        let elsewhere = Scratch::new("elsewhere");
        std::os::unix::fs::symlink(&elsewhere.0, runtime.0.join("athanor")).unwrap();
        let err = socket_dir(Some(runtime.0.clone().into()), "leaf").unwrap_err();
        assert!(matches!(err, LaunchError::Socket(_)), "{err}");
        assert!(!elsewhere.0.join("leaf").exists());
    }

    #[test]
    fn a_missing_runtime_directory_is_refused() {
        for runtime in [None, Some(OsString::new()), Some("relative/run".into())] {
            let err = socket_dir(runtime.clone(), "leaf").unwrap_err();
            assert!(matches!(err, LaunchError::Socket(_)), "{runtime:?}: {err}");
        }
    }
}
