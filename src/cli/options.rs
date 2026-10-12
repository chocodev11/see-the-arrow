use clap::Args;

#[derive(Args, Debug, Clone, Default)]
pub struct TargetOptions {
    /// Point at X,Y in global top-left coordinates (e.g. 760,500).
    #[arg(long, value_name = "x,y", allow_hyphen_values = true)]
    pub at: Option<String>,

    /// Highlight the rect x,y,w,h with a box and point at it.
    #[arg(long, value_name = "x,y,w,h", allow_hyphen_values = true)]
    pub rect: Option<String>,

    /// Point at the mouse cursor.
    #[arg(long)]
    pub mouse: bool,

    /// Point at an app's front window, optionally by title: 'Notepad' or 'Google Chrome:Inbox'.
    #[arg(long, value_name = "app[:title]")]
    pub window: Option<String>,

    /// Point inside the window at: center, title, top-left, top-right, bottom-left, bottom-right.
    #[arg(long, value_name = "anchor", default_value = "center")]
    pub anchor: String,

    /// Point at the UI element with this label (UI Automation on Windows, AT-SPI on Linux).
    #[arg(long, value_name = "label")]
    pub element: Option<String>,

    /// The app the target is in, 'App' or 'App:window or tab title'. It comes to the front first,
    /// and the arrow hides while another app covers the target. With --element: the app to search
    /// (default: the frontmost app).
    #[arg(long, value_name = "app[:title]")]
    pub app: Option<String>,

    /// Only match --element elements with this role, e.g. button.
    #[arg(long, value_name = "role")]
    pub role: Option<String>,

    /// With --at or --rect: coordinates are relative to display N (1-based, see 'bigarrow doctor').
    #[arg(long, value_name = "n", allow_negative_numbers = true)]
    pub display: Option<i64>,

    /// Leave the windows as they are; by default the target's app and window come to the front.
    #[arg(long = "no-raise")]
    pub no_raise: bool,
}

#[derive(Args, Debug, Clone)]
pub struct LookOptions {
    /// The sign. Write a full sentence: 'Click Allow'. {{value}} shows value with a copy button.
    #[arg(long, value_name = "text")]
    pub text: String,

    /// Preferred side for the sign (another side if it does not fit there): auto, top-left, top,
    /// top-right, right, bottom-right, bottom, bottom-left, left.
    #[arg(long, value_name = "side", default_value = "auto")]
    pub from: String,

    /// S, M or L.
    #[arg(long, value_name = "size", default_value = "M")]
    pub size: String,

    /// arrow, ring or box.
    #[arg(long, value_name = "style", default_value = "arrow")]
    pub style: String,

    /// red, orange, yellow, green, teal, blue, purple, pink, black, white, or hex like #FF3B1F / #F31.
    #[arg(long, value_name = "colour", default_value = "red")]
    pub color: String,

    /// Border colour (default: white, dark on light arrows).
    #[arg(long = "border-color", value_name = "colour")]
    pub border_color: Option<String>,

    /// Sign text colour (default: white, dark on light arrows).
    #[arg(long = "text-color", value_name = "colour")]
    pub text_color: Option<String>,

    /// Thin line colour of --border white-black and black (default: black).
    #[arg(long = "edge-color", value_name = "colour")]
    pub edge_color: Option<String>,

    /// Circle of the --close-button X (default: the border colour).
    #[arg(long = "close-color", value_name = "colour")]
    pub close_color: Option<String>,

    /// The X itself (default: the arrow colour).
    #[arg(long = "close-x-color", value_name = "colour")]
    pub close_x_color: Option<String>,

    /// Shaft shape: bend, straight, zigzag or spiral (once around the sign, then to the target).
    #[arg(long, value_name = "shape", default_value = "bend")]
    pub shape: String,

    /// Sign corners: round or sharp.
    #[arg(long, value_name = "corners", default_value = "round")]
    pub corners: String,

    /// shadow (white border and drop shadow), white-black (white border, thin black edge) or black
    /// (thin black outline).
    #[arg(long, value_name = "border", default_value = "shadow")]
    pub border: String,

    /// Show the final frame at once, no draw-on, pulse or fade.
    #[arg(long = "no-animation")]
    pub no_animation: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct BehaviourOptions {
    /// Seconds to show the arrow (point: 8, start/--close-button/--until-click: 300). 0 = until stopped.
    #[arg(long, value_name = "seconds", allow_negative_numbers = true)]
    pub duration: Option<f64>,

    /// Return at once and leave the arrow up in the background; prints the pid.
    #[arg(long)]
    pub detach: bool,

    /// Show an X in the sign. A click on the sign or the shaft always removes the arrow.
    #[arg(long = "close-button")]
    pub close_button: bool,

    /// Dismiss when the human clicks the target.
    #[arg(long = "until-click")]
    pub until_click: bool,

    /// Dismiss on the next click anywhere.
    #[arg(long = "until-any-click")]
    pub until_any_click: bool,

    /// Speak the sign aloud, for when the human is not looking.
    #[arg(long)]
    pub say: bool,

    /// Voice for --say: part of a voice name (Windows: e.g. 'Zira' or 'An'; Linux: spd-say/espeak-ng voice).
    #[arg(long, value_name = "voice")]
    pub voice: Option<String>,

    /// Follow --window or --element as it moves and move the arrow with it.
    #[arg(long)]
    pub follow: bool,

    /// Print a JSON result to stdout (errors as JSON on stderr).
    #[arg(long)]
    pub json: bool,

    /// Resolve the target and plan the layout, draw nothing.
    #[arg(long = "dry-run")]
    pub dry_run: bool,

    /// Render the arrow into this PNG (transparent background) instead of showing it.
    #[arg(long, value_name = "file")]
    pub png: Option<String>,
}

#[derive(Args, Debug, Clone)]
pub struct PointArgs {
    #[command(flatten)]
    pub target: TargetOptions,
    #[command(flatten)]
    pub look: LookOptions,
    #[command(flatten)]
    pub behaviour: BehaviourOptions,
}

#[derive(Args, Debug, Clone)]
pub struct ClearArgs {
    /// Remove every live arrow.
    #[arg(long)]
    pub all: bool,

    /// Remove the arrow with this pid.
    #[arg(long, value_name = "pid")]
    pub pid: Option<u32>,

    /// Remove every arrow drawn from this agent session.
    #[arg(long, value_name = "id")]
    pub session: Option<String>,

    /// Read {"session_id"} from stdin (a Claude Code hook), clear that session's arrows, print nothing.
    #[arg(long)]
    pub hook: bool,

    /// Print a JSON result.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct FrontArgs {
    /// App name (product name or executable).
    #[arg(long, value_name = "name")]
    pub app: String,

    /// Also raise the app's window, or select its tab, whose title contains this text.
    #[arg(long, value_name = "title")]
    pub window: Option<String>,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ElementsArgs {
    /// App name (default: the frontmost app).
    #[arg(long, value_name = "name")]
    pub app: Option<String>,

    /// Only elements whose label contains this text.
    #[arg(long = "match", value_name = "text")]
    pub matching: Option<String>,

    /// Only elements with this role, e.g. button.
    #[arg(long, value_name = "role")]
    pub role: Option<String>,

    /// Print JSON (the same shape the element matcher reads).
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct ScreenshotArgs {
    /// Capture an app's front window, optionally by title: 'Blender' or 'Google Chrome:Inbox'.
    /// It comes to the front first.
    #[arg(long, value_name = "app[:title]")]
    pub window: Option<String>,

    /// Capture the rect x,y,w,h in global top-left coordinates.
    #[arg(long, value_name = "x,y,w,h", allow_hyphen_values = true)]
    pub rect: Option<String>,

    /// Capture display N (1-based, see 'bigarrow doctor'); with --rect, the rect is relative to it.
    #[arg(long, value_name = "n", allow_negative_numbers = true)]
    pub display: Option<i64>,

    /// The PNG to write.
    #[arg(long, value_name = "file")]
    pub png: String,

    /// Shrink the image until its longer side is at most this many pixels. Vision models shrink
    /// large images on their own, which shifts every coordinate read from them.
    #[arg(long = "max-size", value_name = "pixels", value_parser = clap::value_parser!(u32).range(16..))]
    pub max_size: Option<u32>,

    /// With --window: leave the windows as they are.
    #[arg(long = "no-raise")]
    pub no_raise: bool,

    /// Print JSON.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DoctorArgs {
    /// Print JSON.
    #[arg(long)]
    pub json: bool,

    /// Exit 4 unless this capability is there: accessibility, screen-recording.
    #[arg(long, value_name = "permission")]
    pub require: Vec<String>,

    /// Open the system's accessibility settings, where they exist.
    #[arg(long = "open-settings")]
    pub open_settings: bool,

    /// Turn on the accessibility bus for apps started from now on (Linux).
    #[arg(long = "request-accessibility")]
    pub request_accessibility: bool,
}

#[derive(Args, Debug, Clone)]
pub struct DemoArgs {
    /// Seconds per step.
    #[arg(long, value_name = "seconds", default_value_t = 3.0)]
    pub step: f64,
}

#[derive(Args, Debug, Clone)]
pub struct InstallSkillArgs {
    /// Only install for Claude Code.
    #[arg(long)]
    pub claude: bool,

    /// Only install for Codex.
    #[arg(long)]
    pub codex: bool,

    /// Overwrite skill files that differ from the shipped ones.
    #[arg(long)]
    pub force: bool,

    /// The skill folder to install (default: the one built into bigarrow).
    #[arg(long, value_name = "path")]
    pub source: Option<String>,
}
