//! An interactive torture bench for performance, security and correctness:
//! pick a scenario, change its size, toggle the per-frame churn and watch
//! the frame time, the update rate and the checks.
//!
//! - **Performance:** large and churning documents, all-dirty rebuilds,
//!   restyle storms, indented Markdown, localized rows (cached and
//!   per-value Fluent paths), many UIs, `calc()` layout, a selector-heavy
//!   nested list (content reuse must keep working), the text and box
//!   properties of ADR 0027/0028.
//! - **Security:** pathological inputs that once crashed, hung or injected
//!   (nested CSS rules, a 5 000-deep DOM, `var()` and grid bombs, hostile
//!   Fluent arguments, args-JSON injection, remote asset sources, Markdown
//!   bombs, selector, sibling and character-reference bombs). Each must
//!   survive, with a check showing why.
//! - **Correctness:** incremental paths against from-scratch ones (a fresh
//!   twin; a forced full restyle), a metamorphic reversal (order
//!   independence), reuse and quiet-when-idle checks, and an N vs 4N
//!   scaling check — review_0005's methods (see the README).
//!
//! The workloads are generated at runtime into an in-memory asset source
//! (`testbench://`), so they go through the real loaders, pre-scans and
//! all, in the real Bevy frame loop. With `--features hotpath` every
//! pipeline system and the bench's own work is timed per call; run
//! `hotpath console` in another terminal for the live view, or read the
//! report printed on exit.
//!
//! `cargo run --release --example testbench --features hotpath` — see the
//! README for the controls and the scripted form
//! (`-- --scenario big-document --size 2000 --no-render --exit-after 600`).

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::OnceLock;

use bevy::asset::io::memory::{Dir, MemoryAssetReader};
use bevy::asset::io::{AssetSourceBuilder, ErasedAssetReader};
use bevy::diagnostic::FrameCount;
use bevy::input_focus::{FocusCause, InputFocus, InputFocusVisible};
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use bevy::ui_widgets::ScrollArea;
use bevy_markup::prelude::*;
use bevy_markup::{WindowModel, WindowSpacer};
use serde_json::{Value, json};

#[path = "../shared/input.rs"]
mod input;
use input::{Hotkeys, SettingStep};

#[path = "../shared/harness.rs"]
mod harness;

mod scenarios;
use scenarios::{Changed, Model, Role, SIZES, TwinKind};

/// The in-memory files behind `testbench://` (shared with the asset
/// source's reader, so files written at runtime are loadable at once).
static GENERATED: OnceLock<Dir> = OnceLock::new();

fn generated() -> &'static Dir {
    GENERATED.get_or_init(|| Dir::new(PathBuf::new()))
}

/// Registers `testbench://` — before `AssetPlugin`, as Bevy requires.
fn register_generated_source(app: &mut App) {
    let dir = generated().clone();
    app.register_asset_source(
        "testbench",
        AssetSourceBuilder::new(move || -> Box<dyn ErasedAssetReader> {
            Box::new(MemoryAssetReader { root: dir.clone() })
        }),
    );
}

#[hotpath::main]
fn main() {
    harness::run(
        harness::ExampleWindow {
            title: "bevy_markup: testbench",
            resolution: (1600, 900),
            before_plugins: register_generated_source,
            ..Default::default()
        },
        |app| {
            app.insert_resource(Bench::from_args())
                .init_resource::<Stage>()
                .init_resource::<Stats>()
                .init_resource::<Checks>()
                .init_resource::<Twins>()
                .init_resource::<Built>()
                .init_resource::<Scaling>()
                .define_html_element("stage", connect_stage)
                .add_observer(count_builds)
                .add_observer(count_restyles)
                .add_systems(Startup, setup)
                .add_systems(Last, summarize_on_exit)
                .add_systems(
                    Update,
                    (
                        handle_signals,
                        apply_setting_steps,
                        hotkeys,
                        start_scenario,
                        churn,
                        scroll_workloads,
                        move_focus,
                        compare_twins,
                        check_scenario,
                        update_stats,
                        show_shell,
                    )
                        .chain(),
                );
        },
    );
}

/// What the bench runs and the model its workloads render.
#[derive(Resource)]
struct Bench {
    scenarios: Vec<&'static str>,
    active: usize,
    /// Index into [`SIZES`].
    size: usize,
    churn: bool,
    /// `--scenario`: the scenario is fixed (scripted runs; the injected
    /// keys of `--exit-after` must not switch it).
    pinned: bool,
    /// `--size`: the size is fixed too — the injected keys (arrows,
    /// Enter on a focused − / + button) stepped it, so a scripted
    /// `--size 2000` run measured some other size.
    size_pinned: bool,
    /// A start (or restart) is due; it waits for the stage to exist.
    restart: bool,
    generation: u32,
    model: Model,
    /// Churn held while two UIs are compared.
    paused: bool,
    /// The frame of the current start.
    started: u32,
    /// The worst frame time since the start, past the warm-up.
    worst_ms: f32,
    /// The scenario's UIs sit side by side (the stage's `row` class).
    row: bool,
    /// ADR 0031: `--windowed` — the workloads build only their scroll
    /// window (the big-document/restyle classes of scenarios show the
    /// win).
    windowed: bool,
    /// ADR 0031: `--scroll` — advance the main workload's scroll
    /// position every frame (bouncing), so the windowed band moves:
    /// the scroll-frame cost is the measured quantity.
    scroll: bool,
}

impl Bench {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let value = |flag: &str| {
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|index| args.get(index + 1))
                .cloned()
        };
        let scenarios = scenarios::all();
        let pinned = value("--scenario");
        let active = match &pinned {
            Some(id) => scenarios.iter().position(|s| s == id).unwrap_or_else(|| {
                warn!("testbench: unknown --scenario {id}; known: {scenarios:?}");
                0
            }),
            None => 0,
        };
        let size = value("--size")
            .and_then(|size| size.parse::<usize>().ok())
            .map_or(4, |size| {
                SIZES
                    .iter()
                    .position(|step| *step >= size)
                    .unwrap_or(SIZES.len() - 1)
            });
        Self {
            scenarios,
            active,
            size,
            churn: !args.iter().any(|arg| arg == "--no-churn"),
            windowed: args.iter().any(|arg| arg == "--windowed"),
            scroll: args.iter().any(|arg| arg == "--scroll"),
            pinned: pinned.is_some(),
            size_pinned: value("--size").is_some(),
            restart: false,
            generation: 0,
            model: Model::default(),
            paused: false,
            started: 0,
            worst_ms: 0.0,
            row: false,
        }
    }

    fn id(&self) -> &'static str {
        self.scenarios[self.active]
    }

    fn select(&mut self, index: usize) {
        if !self.pinned {
            self.active = index % self.scenarios.len();
            self.restart = true;
        }
    }

    fn step_size(&mut self, steps: i32) {
        if self.size_pinned {
            return;
        }
        let size = (self.size as i32 + steps).clamp(0, SIZES.len() as i32 - 1) as usize;
        if size != self.size {
            self.size = size;
            self.restart = true;
        }
    }
}

/// The shell's stage element (`is="stage"`): the workloads' parent.
#[derive(Resource, Default)]
struct Stage(Option<Entity>);

#[derive(Component)]
struct ShellUi;

/// A workload UI: what it is for and what it loaded.
#[derive(Component)]
struct Workload {
    role: Role,
    template: Handle<HtmlTemplate>,
    sheet: Option<Handle<Stylesheet>>,
}

/// A fresh build spawned only to be compared, then despawned.
#[derive(Component)]
struct FreshTwin;

/// The frame each UI last built or restyled.
#[derive(Resource, Default)]
struct Built(HashMap<Entity, u32>);

#[derive(Resource)]
struct Stats {
    frame_ms: VecDeque<f32>,
    builds: u32,
    restyles: u32,
    builds_rate: u32,
    restyles_rate: u32,
    window_start: f32,
    /// When the shell next shows the numbers (a few times a second: the
    /// shell's own re-render is in the frame time too).
    shell_at: f32,
    /// Show now (a control changed).
    shell_now: bool,
}

impl Default for Stats {
    fn default() -> Self {
        Self {
            frame_ms: VecDeque::with_capacity(120),
            builds: 0,
            restyles: 0,
            builds_rate: 0,
            restyles_rate: 0,
            window_start: 0.0,
            shell_at: 0.0,
            shell_now: true,
        }
    }
}

struct Check {
    id: &'static str,
    state: &'static str,
    detail: String,
}

#[derive(Resource, Default)]
struct Checks(Vec<Check>);

/// The twin comparison in progress, and its tally.
#[derive(Resource, Default)]
struct Twins {
    pending: Option<Pending>,
    matches: u32,
    mismatches: u32,
    last_mismatch: String,
}

struct Pending {
    kind: TwinKind,
    main: Entity,
    twin: Entity,
    /// Spawned for this comparison (despawned after it).
    fresh: bool,
    since: u32,
    /// [`TwinKind::FullRestyle`]: the signature before the forced full
    /// restyle (`None` until it is taken and the restyle is triggered).
    before: Option<Vec<String>>,
}

/// The scaling scenario's measurement (review_0005's method: best of
/// three at N and at 4N, the larger at most 8× the smaller — I03 and I04
/// were quadratic). After a warm-up, phases of [`PHASE`] frames alternate
/// which UI churns (the small one renders N rows, the large one 4N); each
/// phase's mean frame time counts, the best of each kind compares.
#[derive(Resource, Default)]
struct Scaling {
    small: Vec<f32>,
    large: Vec<f32>,
    current: Vec<f32>,
    phase: Option<usize>,
}

/// Frames per scaling phase, the warm-up before the first, the phases.
const PHASE: u32 = 30;
const SCALING_WARMUP: u32 = 60;
const SCALING_PHASES: usize = 6;

/// The UI the scaling scenario churns `since` frames after its start:
/// even phases the small one, odd phases the large one.
fn scaling_phase(since: u32) -> Option<usize> {
    let phase = since.checked_sub(SCALING_WARMUP)? / PHASE;
    (phase < SCALING_PHASES as u32).then_some(phase as usize)
}

/// Both locales, preloaded.
#[derive(Resource)]
struct Languages(Vec<Handle<LocaleBundle>>);

fn setup(mut commands: Commands, asset_server: Res<AssetServer>, mut fonts: ResMut<FontFamilies>) {
    commands.spawn(Camera2d);
    fonts
        .insert("System Sans", FontFaces::new(FontSource::sans_serif()))
        .insert("System Mono", FontFaces::new(FontSource::monospace()))
        .set_generic(GenericFamily::SansSerif, "System Sans")
        .set_generic(GenericFamily::Monospace, "System Mono");
    let languages: Vec<Handle<LocaleBundle>> = ["en-US", "de"]
        .iter()
        .map(|id| asset_server.load(format!("testbench/locales/{id}/main.ftl.ron")))
        .collect();
    commands.insert_resource(ActiveLocale::new(languages[0].clone()));
    commands.insert_resource(Languages(languages));
    commands.spawn((
        ShellUi,
        HtmlUi::new(asset_server.load("testbench/shell.html")),
        HtmlStylesheet(asset_server.load("testbench/style.css")),
        TemplateContext::new(),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
    ));
}

/// `is="stage"`: the shell built its stage; (re)start the scenario in it.
fn connect_stage(stage: In<ElementConnected>, mut slot: ResMut<Stage>, mut bench: ResMut<Bench>) {
    slot.0 = Some(stage.entity);
    bench.restart = true;
}

fn count_builds(
    built: On<HtmlUiBuilt>,
    shells: Query<(), With<ShellUi>>,
    frames: Res<FrameCount>,
    mut stats: ResMut<Stats>,
    mut last: ResMut<Built>,
) {
    if !shells.contains(built.entity) {
        stats.builds += 1;
        last.0.insert(built.entity, frames.0);
    }
}

fn count_restyles(
    restyled: On<HtmlUiRestyled>,
    shells: Query<(), With<ShellUi>>,
    frames: Res<FrameCount>,
    mut stats: ResMut<Stats>,
    mut last: ResMut<Built>,
) {
    if !shells.contains(restyled.entity) {
        stats.restyles += 1;
        last.0.insert(restyled.entity, frames.0);
    }
}

/// The shell's controls.
fn handle_signals(
    mut signals: MessageReader<ElementSignal>,
    mut bench: ResMut<Bench>,
    mut stats: ResMut<Stats>,
    languages: Res<Languages>,
    mut active: ResMut<ActiveLocale>,
) {
    for signal in signals.read() {
        if signal.trigger != SignalTrigger::Click {
            continue;
        }
        match signal.name.as_ref() {
            "scenario" => {
                let id = signal.payload.get("id").and_then(Value::as_str);
                if let Some(index) = id.and_then(|id| bench.scenarios.iter().position(|s| *s == id))
                {
                    bench.select(index);
                }
            }
            "size-down" => bench.step_size(-1),
            "size-up" => bench.step_size(1),
            "churn" => bench.churn ^= true,
            "restart" => bench.restart = true,
            "language" => switch_language(&languages, &mut active),
            _ => continue,
        }
        stats.shell_now = true;
    }
}

/// ← → on the focused size value.
fn apply_setting_steps(
    mut steps: MessageReader<SettingStep>,
    mut bench: ResMut<Bench>,
    mut stats: ResMut<Stats>,
) {
    for step in steps.read() {
        if step.setting == "size" {
            bench.step_size(step.steps);
            stats.shell_now = true;
        }
    }
}

/// Keyboard and gamepad shortcuts.
fn hotkeys(
    hotkeys: Hotkeys,
    mut bench: ResMut<Bench>,
    mut stats: ResMut<Stats>,
    languages: Res<Languages>,
    mut active: ResMut<ActiveLocale>,
) {
    let count = bench.scenarios.len();
    let before = (bench.active, bench.size, bench.churn, bench.restart);
    if hotkeys.just_pressed(KeyCode::KeyN, GamepadButton::RightTrigger) {
        let next = bench.active + 1;
        bench.select(next);
    }
    if hotkeys.just_pressed(KeyCode::KeyP, GamepadButton::LeftTrigger) {
        let previous = bench.active + count - 1;
        bench.select(previous);
    }
    if hotkeys.just_pressed(KeyCode::BracketLeft, GamepadButton::LeftTrigger2) {
        bench.step_size(-1);
    }
    if hotkeys.just_pressed(KeyCode::BracketRight, GamepadButton::RightTrigger2) {
        bench.step_size(1);
    }
    if hotkeys.just_pressed(KeyCode::KeyC, GamepadButton::West) {
        bench.churn ^= true;
    }
    if hotkeys.just_pressed(KeyCode::KeyR, GamepadButton::Select) {
        bench.restart = true;
    }
    if hotkeys.just_pressed(KeyCode::KeyL, GamepadButton::North) {
        switch_language(&languages, &mut active);
        stats.shell_now = true;
    }
    if before != (bench.active, bench.size, bench.churn, bench.restart) {
        stats.shell_now = true;
    }
}

fn switch_language(languages: &Languages, active: &mut ActiveLocale) {
    let current = languages
        .0
        .iter()
        .position(|bundle| active.0.as_ref() == Some(bundle))
        .unwrap_or(0);
    active.set(languages.0[(current + 1) % languages.0.len()].clone());
}

/// A JSON object as a Tera context.
fn template_context(value: &Value) -> TemplateContext {
    let mut context = TemplateContext::new();
    if let Some(map) = value.as_object() {
        for (key, value) in map {
            context = context.with(key.clone(), value);
        }
    }
    context
}

/// Spawns the scenario's workload UIs into the stage.
#[hotpath::measure]
#[allow(clippy::too_many_arguments)]
fn start_scenario(
    mut bench: ResMut<Bench>,
    stage: Res<Stage>,
    workloads: Query<Entity, With<Workload>>,
    server: Res<AssetServer>,
    frames: Res<FrameCount>,
    mut checks: ResMut<Checks>,
    mut twins: ResMut<Twins>,
    mut built: ResMut<Built>,
    mut scaling: ResMut<Scaling>,
    mut commands: Commands,
) {
    if !bench.restart {
        return;
    }
    let Some(stage) = stage.0 else {
        return;
    };
    bench.restart = false;
    for workload in &workloads {
        commands.entity(workload).despawn();
    }
    bench.generation += 1;
    let id = bench.id();
    debug!(
        "testbench: start {id} #{} (size {})",
        bench.generation, SIZES[bench.size]
    );
    bench.model = Model::new(scenarios::model_rows(id, SIZES[bench.size]));
    bench.paused = false;
    bench.started = frames.0;
    bench.worst_ms = 0.0;
    let prefix = format!("g{}-{id}", bench.generation);
    let plan = scenarios::plan(id, generated(), &prefix);
    // `--windowed` adds the band check to every scenario (ADR 0031).
    let windowed_checks: &[&'static str] = if bench.windowed { &["band"] } else { &[] };
    checks.0 = plan
        .checks
        .iter()
        .chain(windowed_checks)
        .map(|id| Check {
            id,
            state: "wait",
            detail: String::new(),
        })
        .collect();
    *twins = Twins::default();
    *scaling = Scaling::default();
    built.0.clear();
    // The workload UIs are nested `HtmlUi` roots right in the stage: the
    // shell keeps those across its own updates (an app node there would be
    // reconciled away). The stage's `row` class lays them side by side.
    bench.row = plan.row;
    for spec in plan.specs {
        // The many-UIs scenario's one card spec stands for N cards.
        let roles: Vec<Role> = match spec.role {
            Role::Card(_) => (0..bench.model.rows.len()).map(Role::Card).collect(),
            role => vec![role],
        };
        for role in roles {
            let template = server.load::<HtmlTemplate>(spec.template.clone());
            let sheet = spec
                .stylesheet
                .clone()
                .map(|path| server.load::<Stylesheet>(path));
            let context = spec
                .context
                .clone()
                .unwrap_or_else(|| scenarios::context(id, &bench.model, role));
            // `--windowed`: a fixed-height scrolling viewport so the
            // windowed build has something to window (ADR 0031).
            let node = if bench.windowed {
                Node {
                    width: match role {
                        Role::Card(_) => Val::Px(110.0),
                        _ if plan.row => Val::Percent(49.0),
                        _ => Val::Percent(100.0),
                    },
                    height: Val::Px(600.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow {
                        x: OverflowAxis::Visible,
                        y: OverflowAxis::Scroll,
                    },
                    ..default()
                }
            } else {
                Node {
                    width: match role {
                        Role::Card(_) => Val::Px(110.0),
                        _ if plan.row => Val::Percent(49.0),
                        _ => Val::Percent(100.0),
                    },
                    flex_direction: FlexDirection::Column,
                    ..default()
                }
            };
            let mut ui = commands.spawn((
                HtmlUi::new(template.clone()),
                template_context(&context),
                node,
                Workload {
                    role,
                    template,
                    sheet: sheet.clone(),
                },
                ChildOf(stage),
            ));
            if let Some(sheet) = sheet {
                ui.insert(HtmlStylesheet(sheet));
            }
            if bench.windowed {
                // The windowed viewport scrolls: the wheel bubbles from
                // the rows to the root's `ScrollArea` observer (bevy's
                // widget; the scripted `--scroll` sweep is independent).
                ui.insert((
                    HtmlWindowed::default(),
                    ScrollPosition::default(),
                    ScrollArea,
                ));
            }
            // The focus scenario's rows take the focus (the churn moves
            // it); every other workload stays out of the shell's
            // navigation.
            if id != "restyle-vs-full" {
                ui.insert(HtmlNoFocus);
            }
        }
    }
}

/// `--scroll`: advance the main workload's scroll position every frame
/// (bouncing over the content's full range), so the windowed band
/// moves — the scroll-frame cost is the measured quantity (ADR 0031).
#[hotpath::measure]
fn scroll_workloads(
    bench: Res<Bench>,
    frames: Res<FrameCount>,
    mut workloads: Query<
        (
            Entity,
            &Workload,
            &mut ScrollPosition,
            &ComputedNode,
            &Children,
        ),
        Without<FreshTwin>,
    >,
    spacers: Query<&WindowSpacer>,
    models: Query<&WindowModel>,
) {
    if !bench.scroll || bench.restart {
        return;
    }
    for (entity, workload, mut scroll, computed, children) in &mut workloads {
        if !matches!(workload.role, Role::Main) {
            continue;
        }
        // ~9000 px/s: 150 px per 60 fps frame — a full 20000 px document
        // sweeps in ~45 s, deep enough to expose band-mapping drift. The
        // scroll position is logical; the computed sizes are physical.
        let scale = computed.inverse_scale_factor;
        let range = ((computed.content_size.y - computed.size.y) * scale).max(0.0);
        let sweep = (frames.0 as f32 * 150.0) % (2.0 * range.max(1.0));
        scroll.0.y = if sweep < range {
            sweep
        } else {
            2.0 * range - sweep
        };
        // CULLPROBE: every 60 frames, the materialized row count against
        // the scroll offset and the content extent, plus the model.
        if frames.0 % 60 == 0 {
            let materialized = children
                .iter()
                .filter(|child| !spacers.contains(*child))
                .count();
            let content = computed.content_size.y;
            let model = models.get(entity).ok().map(|model| model.probe());
            info!(
                "CULLPROBE frame={} scroll={:.0} layout_scroll={:.0} range={:.0} content={:.0} materialized={} spacers={} root={} model={:?}",
                frames.0,
                scroll.0.y,
                computed.scroll_position.y,
                range,
                content,
                materialized,
                children.len() - materialized,
                computed.size,
                model
            );
        }
    }
}

/// One frame of the scenario's churn, written into the workloads.
#[hotpath::measure]
fn churn(
    mut bench: ResMut<Bench>,
    frames: Res<FrameCount>,
    mut workloads: Query<(&Workload, &mut TemplateContext, &mut Node), Without<FreshTwin>>,
) {
    if !bench.churn || bench.paused || bench.restart {
        return;
    }
    let id = bench.id();
    let changed = scenarios::churn(id, &mut bench.model);
    let bench = &*bench;
    let update = |role: Role, context: &mut TemplateContext| {
        *context = hotpath::measure_block!("testbench::serialize_context", {
            template_context(&scenarios::context(id, &bench.model, role))
        });
    };
    // The scaling scenario churns one of its UIs per phase, the other
    // hidden (`display: none` skips its layout, which would otherwise
    // dilute the ratio with a constant).
    let scaling = (id == "scaling").then(|| {
        scaling_phase(frames.0.saturating_sub(bench.started)).map(|phase| {
            if phase % 2 == 0 {
                Role::Main
            } else {
                Role::Twin
            }
        })
    });
    if let Some(Some(active)) = scaling {
        for (workload, _, mut node) in &mut workloads {
            let display = if workload.role == active {
                Display::Flex
            } else {
                Display::None
            };
            if node.display != display {
                node.display = display;
            }
        }
    }
    match changed {
        Changed::None => {}
        Changed::All => {
            for (workload, mut context, _) in &mut workloads {
                let churns = match scaling {
                    Some(role) => role == Some(workload.role),
                    None => matches!(workload.role, Role::Main | Role::Twin),
                };
                if churns {
                    update(workload.role, &mut context);
                }
            }
        }
        Changed::Card(index) => {
            for (workload, mut context, _) in &mut workloads {
                if workload.role == Role::Card(index) {
                    update(workload.role, &mut context);
                }
            }
        }
    }
}

/// The focus scenario's churn: the focus moves to a random row each frame
/// (shown, so `:focus-visible` holds), which restyles in place — scoped
/// to the flipped rows and their siblings (ADR 0029).
fn move_focus(
    mut bench: ResMut<Bench>,
    workloads: Query<Entity, (With<Workload>, Without<FreshTwin>)>,
    tree: Query<&Children>,
    focusables: Query<(), With<Focusable>>,
    mut focus: ResMut<InputFocus>,
    mut visible: ResMut<InputFocusVisible>,
) {
    if bench.id() != "restyle-vs-full" || !bench.churn || bench.paused || bench.restart {
        return;
    }
    let candidates: Vec<Entity> = workloads
        .iter()
        .flat_map(|root| tree.iter_descendants_depth_first(root).collect::<Vec<_>>())
        .filter(|entity| focusables.contains(*entity))
        .collect();
    if candidates.is_empty() {
        return;
    }
    let pick = bench.model.random(candidates.len());
    focus.set(candidates[pick], FocusCause::Navigated);
    visible.0 = true;
}

/// The components a signature reads.
type SignatureParts<'a> = (
    Option<&'a HtmlElement>,
    Option<&'a Text>,
    Option<&'a TextSpan>,
    Option<&'a BackgroundColor>,
    Option<&'a TextColor>,
    Option<&'a TextFont>,
    Option<&'a BorderColor>,
    Option<&'a Node>,
    Option<&'a TextLayout>,
    Option<&'a LetterSpacing>,
);

/// What a UI built, as text: elements (tag, id, classes, display, border
/// color), text runs with their color, size, weight, alignment and
/// spacing, and backgrounds, in document order. Two UIs of the same state
/// must agree — a dump that only held tags and backgrounds missed most
/// of what selectors style.
#[hotpath::measure]
fn signature(root: Entity, tree: &Query<&Children>, parts: &Query<SignatureParts>) -> Vec<String> {
    tree.iter_descendants_depth_first(root)
        .filter_map(|entity| signature_line(entity, parts))
        .collect()
}

/// One entity's signature line (`None`: nothing to compare).
fn signature_line(entity: Entity, parts: &Query<SignatureParts>) -> Option<String> {
    let (element, text, span, background, color, font, border, node, layout, spacing) =
        parts.get(entity).ok()?;
    let mut line = String::new();
    if let Some(element) = element {
        let _ = write!(
            line,
            "<{} {:?} {:?}>",
            element.tag, element.id, element.classes
        );
        if let Some(node) = node {
            let _ = write!(line, " {:?}", node.display);
        }
        if let Some(border) = border.filter(|border| **border != BorderColor::DEFAULT) {
            let _ = write!(line, " border={:?}", border.top);
        }
    }
    if let Some(text) = text {
        let _ = write!(line, "{:?}", text.0);
    }
    if let Some(span) = span {
        let _ = write!(line, "{:?}", span.0);
    }
    if text.is_some() || span.is_some() {
        if let Some(color) = color {
            let _ = write!(line, " {:?}", color.0);
        }
        if let Some(font) = font {
            let _ = write!(line, " {:?} {:?}", font.font_size, font.weight);
        }
        if let Some(layout) = layout {
            let _ = write!(line, " {:?}", layout.justify);
        }
        if let Some(spacing) = spacing {
            let _ = write!(line, " {spacing:?}");
        }
    }
    if let Some(background) = background.filter(|background| background.0 != Color::NONE) {
        let _ = write!(line, " bg={:?}", background.0);
    }
    (!line.is_empty()).then_some(line)
}

/// Per row (a `slot` element, keyed by the row name it shows), the
/// signature of its subtree: the order-independence comparison (row
/// order differs, each row's result must not).
fn row_signatures(
    root: Entity,
    tree: &Query<&Children>,
    parts: &Query<SignatureParts>,
) -> std::collections::BTreeMap<String, Vec<String>> {
    let mut rows = std::collections::BTreeMap::new();
    for entity in tree.iter_descendants_depth_first(root) {
        let Ok((Some(element), ..)) = parts.get(entity) else {
            continue;
        };
        if !element.classes.iter().any(|class| class == "slot") {
            continue;
        }
        let lines: Vec<String> = std::iter::once(entity)
            .chain(tree.iter_descendants_depth_first(entity))
            .filter_map(|entity| signature_line(entity, parts))
            .collect();
        let key = lines
            .iter()
            .find(|line| line.contains("unit-"))
            .cloned()
            .unwrap_or_default();
        rows.insert(key, lines);
    }
    rows
}

/// The correctness scenarios: every 20 frames, hold the churn, let the
/// UIs settle, and compare what they built ([`TwinKind`]).
#[hotpath::measure]
#[allow(clippy::too_many_arguments)]
fn compare_twins(
    mut bench: ResMut<Bench>,
    frames: Res<FrameCount>,
    mut twins: ResMut<Twins>,
    built: Res<Built>,
    workloads: Query<(Entity, &Workload, &TemplateContext, &ChildOf)>,
    tree: Query<&Children>,
    parts: Query<SignatureParts>,
    mut commands: Commands,
) {
    let id = bench.id();
    let Some(kind) = scenarios::twin_kind(id) else {
        return;
    };
    if bench.restart {
        return;
    }
    let now = frames.0;
    let Some(mut pending) = twins.pending.take() else {
        if now.saturating_sub(bench.started) < 30 || now % 20 != 0 {
            return;
        }
        let Some((main, workload, context, parent)) = workloads
            .iter()
            .find(|(_, workload, ..)| workload.role == Role::Main)
        else {
            return;
        };
        let spawn_twin = |context: TemplateContext, commands: &mut Commands| {
            // A fresh UI, laid out nowhere.
            let mut twin = commands.spawn((
                HtmlUi::new(workload.template.clone()),
                context,
                Node {
                    display: Display::None,
                    ..default()
                },
                HtmlNoFocus,
                FreshTwin,
                Workload {
                    role: Role::Twin,
                    template: workload.template.clone(),
                    sheet: workload.sheet.clone(),
                },
                ChildOf(parent.parent()),
            ));
            if let Some(sheet) = &workload.sheet {
                twin.insert(HtmlStylesheet(sheet.clone()));
            }
            twin.id()
        };
        let (twin, fresh) = match kind {
            TwinKind::Fresh => (spawn_twin(context.clone(), &mut commands), true),
            TwinKind::Reversed => {
                // The same rows, reversed (the churn is held, so the model
                // is what the main UI rendered).
                let mut value = scenarios::context(id, &bench.model, Role::Main);
                if let Some(rows) = value.get_mut("rows").and_then(Value::as_array_mut) {
                    rows.reverse();
                }
                (spawn_twin(template_context(&value), &mut commands), true)
            }
            TwinKind::Static => {
                let Some((twin, ..)) = workloads
                    .iter()
                    .find(|(_, workload, ..)| workload.role == Role::Twin)
                else {
                    return;
                };
                (twin, false)
            }
            TwinKind::FullRestyle => (main, false),
        };
        bench.paused = true;
        twins.pending = Some(Pending {
            kind,
            main,
            twin,
            fresh,
            since: now,
            before: None,
        });
        return;
    };
    let last = |entity| built.0.get(&entity).copied();
    let quiet_since = |entity, frames: u32| now.saturating_sub(last(entity).unwrap_or(0)) >= frames;
    let timed_out = now.saturating_sub(pending.since) > 300;
    let settled = match pending.kind {
        TwinKind::FullRestyle => match &pending.before {
            // First the scoped state settles; then the forced full
            // restyle (re-inserting the sheet is a full-page style
            // signal) must land and settle.
            None => {
                if quiet_since(pending.main, 2) && now.saturating_sub(pending.since) >= 3 {
                    pending.before = Some(signature(pending.main, &tree, &parts));
                    if let Ok((_, workload, ..)) = workloads.get(pending.main)
                        && let Some(sheet) = &workload.sheet
                    {
                        commands
                            .entity(pending.main)
                            .insert(HtmlStylesheet(sheet.clone()));
                    }
                    pending.since = now;
                }
                false
            }
            Some(_) => {
                // The forced restyle lands in this frame's PostUpdate.
                last(pending.main).is_some_and(|frame| frame >= pending.since)
                    && quiet_since(pending.main, 2)
            }
        },
        _ => {
            last(pending.twin).is_some_and(|twin| twin >= pending.since)
                && now.saturating_sub(
                    last(pending.twin)
                        .unwrap_or(now)
                        .max(last(pending.main).unwrap_or(0)),
                ) >= 2
                && now.saturating_sub(pending.since) >= 3
        }
    };
    if !settled && !timed_out {
        twins.pending = Some(pending);
        return;
    }
    let mismatch = if timed_out && !settled {
        Some("timeout: the comparison never settled".to_owned())
    } else {
        match pending.kind {
            TwinKind::Reversed => {
                let main = row_signatures(pending.main, &tree, &parts);
                let twin = row_signatures(pending.twin, &tree, &parts);
                main.iter()
                    .find(|(id, lines)| twin.get(*id) != Some(*lines))
                    .map(|(id, lines)| {
                        format!(
                            "{id}: {} / {}",
                            lines.join(" "),
                            twin.get(id)
                                .map_or("(missing)".to_owned(), |lines| lines.join(" "))
                        )
                    })
                    .or_else(|| {
                        (main.len() != twin.len())
                            .then(|| format!("{} rows / {}", main.len(), twin.len()))
                    })
            }
            kind => {
                let main = if kind == TwinKind::FullRestyle {
                    pending.before.clone().unwrap_or_default()
                } else {
                    signature(pending.main, &tree, &parts)
                };
                let twin = signature(pending.twin, &tree, &parts);
                (main != twin).then(|| {
                    let at = main
                        .iter()
                        .zip(&twin)
                        .position(|(a, b)| a != b)
                        .unwrap_or(main.len().min(twin.len()));
                    format!(
                        "line {at}: {} / {}",
                        main.get(at).map_or("(end)", String::as_str),
                        twin.get(at).map_or("(end)", String::as_str)
                    )
                })
            }
        }
    };
    match mismatch {
        None => twins.matches += 1,
        Some(mismatch) => {
            twins.mismatches += 1;
            warn!("testbench: twins differ at {mismatch}");
            twins.last_mismatch = mismatch;
        }
    }
    if pending.fresh {
        commands.entity(pending.twin).despawn();
    }
    bench.paused = false;
}

/// The text a UI element shows (its own text and its spans).
fn element_text(
    entity: Entity,
    tree: &Query<&Children>,
    texts: &Query<(Option<&Text>, Option<&TextSpan>)>,
) -> String {
    std::iter::once(entity)
        .chain(tree.iter_descendants_depth_first(entity))
        .filter_map(|entity| texts.get(entity).ok())
        .map(|(text, span)| {
            format!(
                "{}{}",
                text.map_or("", |text| text.0.as_str()),
                span.map_or("", |span| span.0.as_str())
            )
        })
        .collect()
}

/// Evaluates the scenario's checks.
#[hotpath::measure]
#[allow(clippy::too_many_arguments)]
fn check_scenario(
    bench: Res<Bench>,
    frames: Res<FrameCount>,
    mut checks: ResMut<Checks>,
    twins: Res<Twins>,
    built: Res<Built>,
    server: Res<AssetServer>,
    workloads: Query<(Entity, &Workload, &RenderedHtml), Without<FreshTwin>>,
    tree: Query<&Children>,
    elements: Query<(Entity, &HtmlElement, Option<&ElementSignals>)>,
    texts: Query<(Option<&Text>, Option<&TextSpan>)>,
    skeletons: Query<&bevy_markup::Skeleton>,
    nodes: Query<(Ref<Node>, Option<&ComputedNode>)>,
    paint: Query<(Option<&TextColor>, Option<&BackgroundColor>)>,
    scaling: Res<Scaling>,
    boxes: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    let now = frames.0;
    let since = now.saturating_sub(bench.started);
    let main_ui = workloads
        .iter()
        .find(|(_, workload, _)| workload.role == Role::Main)
        .map(|(entity, ..)| entity);
    let probe = |name: &str| {
        workloads
            .iter()
            .find(|(_, workload, _)| matches!(workload.role, Role::Probe(probe) if probe == name))
    };
    // The workloads' elements (not the shell's).
    let workload_elements = || {
        workloads.iter().flat_map(|(root, ..)| {
            tree.iter_descendants_depth_first(root)
                .filter_map(|entity| elements.get(entity).ok())
                .collect::<Vec<_>>()
        })
    };
    for check in &mut checks.0 {
        let (state, detail): (&'static str, String) = match check.id {
            "budget" => {
                if now.saturating_sub(bench.started) < 60 {
                    ("wait", format!("{:.1}", bench.worst_ms))
                } else if bench.worst_ms < 250.0 {
                    ("pass", format!("{:.1}", bench.worst_ms))
                } else {
                    ("fail", format!("{:.1}", bench.worst_ms))
                }
            }
            "built" => {
                let mut state = "pass";
                let mut detail = String::new();
                for (entity, _, rendered) in &workloads {
                    match rendered {
                        RenderedHtml::Failed(message) => {
                            state = "fail";
                            detail = message.chars().take(80).collect();
                        }
                        RenderedHtml::Ready(_) if built.0.contains_key(&entity) => {}
                        _ if state == "pass" => state = "wait",
                        _ => {}
                    }
                }
                if workloads.is_empty() {
                    state = "wait";
                }
                if state == "pass" {
                    detail = format!("({} elements)", workload_elements().count());
                }
                (state, detail)
            }
            "survived" => {
                let resolved = workloads.iter().all(|(_, workload, rendered)| {
                    !matches!(rendered, RenderedHtml::Pending)
                        || server.load_state(&workload.template).is_failed()
                });
                if resolved && !workloads.is_empty() {
                    (
                        "pass",
                        format!("({} elements)", workload_elements().count()),
                    )
                } else {
                    ("wait", String::new())
                }
            }
            "sheet-refused" => match probe("sheet")
                .and_then(|(_, workload, _)| workload.sheet.as_ref())
            {
                Some(sheet) if server.load_state(sheet).is_failed() => ("pass", String::new()),
                Some(sheet) if server.load_state(sheet).is_loaded() => ("fail", "it loaded".into()),
                _ => ("wait", String::new()),
            },
            "no-forged" => {
                let built_all = workloads
                    .iter()
                    .all(|(entity, ..)| built.0.contains_key(&entity));
                if !built_all || workloads.is_empty() {
                    ("wait", String::new())
                } else {
                    let forged = workload_elements()
                        .filter(|(_, element, signals)| {
                            matches!(element.tag.as_str(), "a" | "img" | "button" | "script")
                                || signals.is_some()
                        })
                        .count();
                    (
                        if forged == 0 { "pass" } else { "fail" },
                        format!("({forged} forged)"),
                    )
                }
            }
            "no-fallback" => {
                // A payload whose arguments failed to parse shows the
                // element's own content, "x" (bug_0128: Tera's `\u{…}`
                // escapes in a printed map).
                let built_all = workloads
                    .iter()
                    .all(|(entity, ..)| built.0.contains_key(&entity));
                if !built_all || workloads.is_empty() {
                    ("wait", String::new())
                } else {
                    let fallbacks = workload_elements()
                        .filter(|(entity, element, _)| {
                            element.tag == "p" && element_text(*entity, &tree, &texts) == "x"
                        })
                        .count();
                    (
                        if fallbacks == 0 { "pass" } else { "fail" },
                        format!("({fallbacks} fell back)"),
                    )
                }
            }
            "map-literal" => {
                let text_of = |id: &str| {
                    workload_elements()
                        .find(|(_, element, _)| element.id.as_deref() == Some(id))
                        .map(|(entity, ..)| element_text(entity, &tree, &texts))
                        .unwrap_or_default()
                };
                let literal = text_of("literal");
                let splice = text_of("splice");
                if literal.is_empty() {
                    ("wait", String::new())
                } else if literal.contains("INJECTED") {
                    ("pass", format!("({literal:?}; splice: {splice:?})"))
                } else {
                    ("fail", format!("({literal:?})"))
                }
            }
            "include-refused" => match probe("include") {
                Some((_, workload, _)) if server.load_state(&workload.template).is_failed() => {
                    ("pass", String::new())
                }
                Some((_, workload, _)) if server.load_state(&workload.template).is_loaded() => {
                    ("fail", "it loaded".into())
                }
                _ => ("wait", String::new()),
            },
            "border-skipped" => match probe("border") {
                Some((entity, workload, _)) => {
                    let sheet = workload
                        .sheet
                        .as_ref()
                        .map(|sheet| server.load_state(sheet));
                    if sheet.as_ref().is_some_and(|state| state.is_failed()) {
                        ("fail", "the sheet failed".into())
                    } else if built.0.contains_key(&entity) {
                        ("pass", String::new())
                    } else {
                        ("wait", String::new())
                    }
                }
                None => ("wait", String::new()),
            },
            // ADR 0021's reuse keeps working (bug_0150: it silently
            // stopped inside containers; every result still matched a
            // fresh build, so only the reuse itself shows it).
            // `last_reuse`, not `pairing_health`: the health starts at
            // `(true, 0)` and passed with bug_0150 put back (no pairing
            // ran at all).
            "reuse" => match main_ui.and_then(|ui| skeletons.get(ui).ok()) {
                _ if since < 90 => ("wait", String::new()),
                Some(skeleton) => match skeleton.last_reuse() {
                    Some((reused, total)) => {
                        let (useful, strike) = skeleton.pairing_health();
                        let detail = format!(
                            "({reused} of {total} reused; useful {useful}, strike {strike})"
                        );
                        // Three quarters: a sparse churn reuses nearly
                        // everything; bug_0150 still reused the rows'
                        // grandchildren (two thirds), so "half" missed it.
                        let pass = reused > 0 && reused * 4 >= total * 3 && useful && strike == 0;
                        (if pass { "pass" } else { "fail" }, detail)
                    }
                    None => ("wait", "(no content update yet)".into()),
                },
                None => ("fail", "no skeleton".into()),
            },
            // `overflow: auto` scrolls exactly when the content overflows,
            // also after in-place updates (review_0005 I09).
            "auto-scroll" if check.state == "fail" => (check.state, check.detail.clone()),
            "auto-scroll" => {
                let panel = workload_elements()
                    .find(|(_, element, _)| element.id.as_deref() == Some("panel"));
                match panel.and_then(|(entity, ..)| nodes.get(entity).ok()) {
                    // Past the warm-up, every frame (a failure sticks:
                    // the promotion may recover on a later layout change).
                    Some((node, Some(computed))) if since >= 60 => {
                        let overflows = computed.content_size.y > computed.size.y + 0.5;
                        let scrolls = node.overflow.y == OverflowAxis::Scroll;
                        let detail = format!(
                            "(content {:.0} / box {:.0}, {:?})",
                            computed.content_size.y, computed.size.y, node.overflow.y
                        );
                        (if overflows == scrolls { "pass" } else { "fail" }, detail)
                    }
                    _ => ("wait", String::new()),
                }
            }
            // An idle UI stays quiet: no build, no restyle and no `Node`
            // write after it settles (the change-loop class: a write that
            // re-triggers its own trigger — review_0005 I09's promotion,
            // review_0003 G15). The churn writes identical contexts.
            "quiet" => {
                if check.state == "fail" {
                    (check.state, check.detail.clone())
                } else if since < 90 {
                    ("wait", String::new())
                } else {
                    let rebuilt = main_ui
                        .and_then(|ui| built.0.get(&ui))
                        .copied()
                        .filter(|frame| frame.saturating_sub(bench.started) > 90);
                    let written = workload_elements()
                        .filter(|(entity, ..)| {
                            nodes.get(*entity).is_ok_and(|(node, _)| node.is_changed())
                        })
                        .count();
                    match (rebuilt, written) {
                        (Some(frame), _) => ("fail", format!("(updated at frame {frame})")),
                        (None, written) if written > 0 => {
                            ("fail", format!("({written} Node writes at frame {now})"))
                        }
                        _ if since >= 150 => {
                            ("pass", format!("(frames {}..{now})", bench.started + 90))
                        }
                        _ => ("wait", String::new()),
                    }
                }
            }
            // Scaling: best of three at N and at 4N (see `Scaling`).
            "linear" => {
                let best = |samples: &[f32]| samples.iter().copied().fold(f32::INFINITY, f32::min);
                if scaling.small.len() < SCALING_PHASES / 2
                    || scaling.large.len() < SCALING_PHASES / 2
                {
                    (
                        "wait",
                        format!("({} / {} phases)", scaling.small.len(), scaling.large.len()),
                    )
                } else {
                    let (small, large) = (best(&scaling.small), best(&scaling.large));
                    let ratio = large / small.max(0.001);
                    let detail = format!("(N {small:.2} ms, 4N {large:.2} ms: {ratio:.1}×)");
                    (if ratio <= 8.0 { "pass" } else { "fail" }, detail)
                }
            }
            // The quoted-value probes matched (bug_0144: a `]`, combinator
            // characters or an escaped quote inside a value dropped the
            // rule).
            "quoted-match" => {
                let painted = |id: &str| {
                    workload_elements()
                        .find(|(_, element, _)| element.id.as_deref() == Some(id))
                        .map(|(entity, ..)| {
                            std::iter::once(entity)
                                .chain(tree.iter_descendants_depth_first(entity))
                                .filter_map(|entity| paint.get(entity).ok())
                                .collect::<Vec<_>>()
                        })
                };
                match (painted("probe"), painted("quotes")) {
                    (Some(probe), Some(quotes)) => {
                        let background = probe.iter().any(|(_, bg)| {
                            bg.is_some_and(|bg| bg.0 == Color::srgb_u8(0x2a, 0x6f, 0x2a))
                        });
                        let color = quotes.iter().any(|(color, _)| {
                            color.is_some_and(|color| color.0 == Color::srgb_u8(0x7f, 0xdb, 0x8f))
                        });
                        let detail = format!("(brackets {background}, quotes {color})");
                        (if background && color { "pass" } else { "fail" }, detail)
                    }
                    _ => ("wait", String::new()),
                }
            }
            // bug_0179: a `style` attribute nesting functions past the
            // paren guard is dropped whole: the sheet's green stays.
            "inline-dropped" => match workload_elements()
                .find(|(_, element, _)| element.id.as_deref() == Some("inline"))
            {
                Some((entity, ..)) => {
                    let green = std::iter::once(entity)
                        .chain(tree.iter_descendants_depth_first(entity))
                        .filter_map(|entity| paint.get(entity).ok())
                        .any(|(color, _)| {
                            color.is_some_and(|color| color.0 == Color::srgb_u8(0x7f, 0xdb, 0x8f))
                        });
                    (if green { "pass" } else { "fail" }, String::new())
                }
                None => ("wait", String::new()),
            },
            // ADR 0034 / bug_0180: the unsupported and panicking selectors
            // skip their rules while the sheet loads (`#ok` is green),
            // `:nth-child(0)` doesn't colour the root (`#plain` keeps the
            // page colour) and a Level 4 selector matches (`#level4`).
            "engine-rules" => {
                let paint_of = |id: &str| {
                    workload_elements()
                        .find(|(_, element, _)| element.id.as_deref() == Some(id))
                        .map(|(entity, ..)| {
                            std::iter::once(entity)
                                .chain(tree.iter_descendants_depth_first(entity))
                                .filter_map(|entity| paint.get(entity).ok())
                                .collect::<Vec<_>>()
                        })
                };
                let colored = |paints: &[(Option<&TextColor>, Option<&BackgroundColor>)],
                               rgb: (u8, u8, u8)| {
                    paints.iter().any(|(color, _)| {
                        color.is_some_and(|color| color.0 == Color::srgb_u8(rgb.0, rgb.1, rgb.2))
                    })
                };
                match (paint_of("ok"), paint_of("plain"), paint_of("level4")) {
                    (Some(ok), Some(plain), Some(level4)) => {
                        let valid = colored(&ok, (0x7f, 0xdb, 0x8f));
                        let root = colored(&plain, (0xdf, 0xe6, 0xee));
                        let level = level4.iter().any(|(_, bg)| {
                            bg.is_some_and(|bg| bg.0 == Color::srgb_u8(0x2a, 0x6f, 0x2a))
                        });
                        let detail = format!("(valid {valid}, root {root}, level 4 {level})");
                        (
                            if valid && root && level {
                                "pass"
                            } else {
                                "fail"
                            },
                            detail,
                        )
                    }
                    _ => ("wait", String::new()),
                }
            }
            // Character references decode as browsers do (review_0005
            // I04's fix also stopped a sign from counting as a digit).
            "decoded" => match workload_elements()
                .find(|(_, element, _)| element.id.as_deref() == Some("decoded"))
            {
                Some((entity, ..)) => {
                    let text = element_text(entity, &tree, &texts);
                    let expected = "AB&&#+65;\u{FFFD}";
                    (
                        if text == expected { "pass" } else { "fail" },
                        format!("({text:?})"),
                    )
                }
                None => ("wait", String::new()),
            },
            // `--windowed`: the built rows cover the main workload's
            // viewport — no gap taller than a row between the viewport's
            // top and the content's end. bug_0151's symptom (the band
            // drifted off the viewport while the built count stayed
            // right) is exactly a gap; a failure sticks with its frame.
            "band" if check.state == "fail" => (check.state, check.detail.clone()),
            "band" => match main_ui.and_then(|ui| boxes.get(ui).ok().map(|root| (ui, root))) {
                Some((ui, (root, transform))) if since >= 60 => {
                    let scale = root.inverse_scale_factor;
                    let top = (transform.translation.y - root.size.y / 2.0
                        + root.border.min_inset.y)
                        * scale;
                    let bottom = (transform.translation.y + root.size.y / 2.0
                        - root.border.max_inset.y)
                        * scale;
                    // Leaf elements only: a container spans its culled
                    // spacers too.
                    let mut spans: Vec<(f32, f32)> = tree
                        .iter_descendants_depth_first(ui)
                        .filter(|entity| elements.contains(*entity))
                        .filter(|entity| {
                            !tree.get(*entity).is_ok_and(|children| {
                                children.iter().any(|child| elements.contains(child))
                            })
                        })
                        .filter_map(|entity| boxes.get(entity).ok())
                        .map(|(node, transform)| {
                            let half = node.size.y / 2.0;
                            (
                                (transform.translation.y - half) * scale,
                                (transform.translation.y + half) * scale,
                            )
                        })
                        .filter(|(a, b)| *b > top && *a < bottom)
                        .collect();
                    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
                    let row = spans
                        .iter()
                        .map(|(a, b)| b - a)
                        .fold(0.0, f32::max)
                        .max(8.0);
                    // The content's end, in the window: past it an empty
                    // viewport is just the document ending.
                    let content_end =
                        top - root.scroll_position.y * scale + root.content_size.y * scale;
                    let end = bottom.min(content_end);
                    let mut cursor = top;
                    let mut gap = 0.0_f32;
                    for (a, b) in &spans {
                        gap = gap.max(a - cursor);
                        cursor = cursor.max(*b);
                    }
                    gap = gap.max(end - cursor);
                    if gap > row {
                        (
                            "fail",
                            format!(
                                "({gap:.0} px uncovered at frame {now}, {} rows in view)",
                                spans.len()
                            ),
                        )
                    } else {
                        ("pass", format!("({} rows in view)", spans.len()))
                    }
                }
                _ => ("wait", String::new()),
            },
            "twins" => {
                let detail = if twins.mismatches > 0 {
                    format!(
                        "({} / {}: {})",
                        twins.matches, twins.mismatches, twins.last_mismatch
                    )
                } else {
                    format!("({} / {})", twins.matches, twins.mismatches)
                };
                let state = if twins.mismatches > 0 {
                    "fail"
                } else if twins.matches > 0 {
                    "pass"
                } else {
                    "wait"
                };
                (state, detail)
            }
            _ => ("wait", String::new()),
        };
        // Scripted runs (`--no-render --exit-after N`) read the checks
        // from the log.
        if state != check.state {
            info!(
                "testbench: {} {}: {} {}",
                bench.id(),
                check.id,
                state,
                detail
            );
        }
        check.state = state;
        check.detail = detail;
    }
}

/// On exit, the final state of every check (a scripted run's result).
fn summarize_on_exit(
    mut exits: MessageReader<AppExit>,
    bench: Res<Bench>,
    checks: Res<Checks>,
    twins: Res<Twins>,
) {
    if exits.read().next().is_none() {
        return;
    }
    for check in &checks.0 {
        info!(
            "testbench: final {} {}: {} {}",
            bench.id(),
            check.id,
            check.state,
            check.detail
        );
    }
    if twins.matches + twins.mismatches > 0 {
        info!(
            "testbench: final {} twins compared: {} match, {} mismatch",
            bench.id(),
            twins.matches,
            twins.mismatches
        );
    }
}

/// Frame time and update rates.
fn update_stats(
    // Real time: the virtual clock clamps a frame to 250 ms, which capped
    // the budget check's worst frame and the scaling phases at exactly
    // that (a 4N frame of a second read as 250 ms).
    time: Res<Time<Real>>,
    frames: Res<FrameCount>,
    mut stats: ResMut<Stats>,
    mut bench: ResMut<Bench>,
    mut scaling: ResMut<Scaling>,
) {
    let ms = time.delta_secs() * 1000.0;
    // The scaling phases: a phase's mean frame time, its first frames
    // skipped (the switch's own rebuild).
    if bench.id() == "scaling" && !bench.restart {
        let phase = scaling_phase(frames.0.saturating_sub(bench.started));
        if phase != scaling.phase {
            if let Some(done) = scaling.phase {
                let samples = std::mem::take(&mut scaling.current);
                let mean = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
                if done % 2 == 0 {
                    scaling.small.push(mean);
                } else {
                    scaling.large.push(mean);
                }
            }
            scaling.phase = phase;
            scaling.current.clear();
        } else if phase.is_some()
            && frames
                .0
                .saturating_sub(bench.started)
                .saturating_sub(SCALING_WARMUP)
                % PHASE
                >= 5
        {
            scaling.current.push(ms);
        }
    }
    if stats.frame_ms.len() == 120 {
        stats.frame_ms.pop_front();
    }
    stats.frame_ms.push_back(ms);
    // Past the warm-up (loads, the first builds), the worst frame counts.
    if frames.0.saturating_sub(bench.started) > 30 && !bench.restart {
        bench.worst_ms = bench.worst_ms.max(ms);
    }
    let now = time.elapsed_secs();
    if now - stats.window_start >= 1.0 {
        stats.builds_rate = stats.builds;
        stats.restyles_rate = stats.restyles;
        stats.builds = 0;
        stats.restyles = 0;
        stats.window_start = now;
    }
}

/// Writes the numbers and the checks into the shell, a few times a second.
#[hotpath::measure]
#[allow(clippy::too_many_arguments)]
fn show_shell(
    time: Res<Time>,
    mut stats: ResMut<Stats>,
    bench: Res<Bench>,
    checks: Res<Checks>,
    elements: Query<(), With<HtmlElement>>,
    workloads: Query<(), (With<Workload>, Without<FreshTwin>)>,
    mut shell: Query<&mut TemplateContext, (With<ShellUi>, Without<Workload>)>,
) {
    let now = time.elapsed_secs();
    if !stats.shell_now && now < stats.shell_at {
        return;
    }
    stats.shell_now = false;
    stats.shell_at = now + 0.25;
    let Ok(mut context) = shell.single_mut() else {
        return;
    };
    let count = stats.frame_ms.len().max(1) as f32;
    let avg = stats.frame_ms.iter().sum::<f32>() / count;
    let max = stats.frame_ms.iter().copied().fold(0.0, f32::max);
    let groups: Vec<Value> = scenarios::GROUPS
        .iter()
        .map(|(id, ids)| json!({ "id": id, "scenarios": ids }))
        .collect();
    let checks: Vec<Value> = checks
        .0
        .iter()
        .map(|check| json!({ "id": check.id, "state": check.state, "detail": check.detail }))
        .collect();
    *context = template_context(&json!({
        "groups": groups,
        "active": bench.id(),
        "size": SIZES[bench.size],
        "churn": bench.churn,
        "row": bench.row,
        "stats": {
            "avg_ms": format!("{avg:.1}"),
            "max_ms": format!("{max:.1}"),
            "fps": format!("{:.0}", 1000.0 / avg.max(0.001)),
            "builds": stats.builds_rate,
            "restyles": stats.restyles_rate,
            "uis": workloads.iter().count(),
            "elements": elements.iter().count(),
        },
        "checks": checks,
    }));
}
