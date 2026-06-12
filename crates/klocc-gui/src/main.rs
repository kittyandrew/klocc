use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap, HashSet},
    env,
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};

use gpui::{
    App, Bounds, Context, FontWeight, MouseButton, MouseDownEvent, MouseMoveEvent, Pixels, Point, Render, SharedString,
    Task, TextAlign, TextRun, Window, WindowBounds, WindowOptions, canvas, div, point, prelude::*, px, quad, rgb, rgba,
    size,
};
use gpui_platform::application;
use klocc_artifact::{Artifact, SourceNode};

mod exercise;
mod layout;
mod modes;
mod theme;

use exercise::exercise_artifact;
use layout::{
    HitTarget, RectNode, TinyGroupSelection, ViewKey, build_layout, has_visible_children, hit_test, human_group,
    human_label, layout_ascii_map, layout_debug_summary, layout_quality, layout_rect_manifest, layout_size_histogram,
    rect_display_text, source_count_for_view,
};
use modes::{ColorMode, CompletenessMode, FilterMode, HierarchyMode, MetricMode, cycle};

fn main() {
    let initial_path = env::args().nth(1).map(PathBuf::from);
    if env::var_os("KLOCC_GUI_EXERCISE").is_some() {
        let Some(path) = initial_path else {
            eprintln!("klocc-gui: pass an artifact path when KLOCC_GUI_EXERCISE is set");
            std::process::exit(2);
        };
        if let Err(error) = exercise_artifact(&path) {
            eprintln!("klocc-gui: exercise failed: {error:#}");
            std::process::exit(1);
        }
        return;
    }

    application().run(move |cx: &mut App| {
        let options = window_options(cx);
        let window = match cx.open_window(options, |window, cx| {
            cx.new(|cx| Viewer::new(initial_path.clone(), window, cx))
        }) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("klocc-gui: failed to open window: {error:#}");
                std::process::exit(1);
            }
        };
        let view = match window.update(cx, |_, _, cx| cx.entity()) {
            Ok(view) => view,
            Err(error) => {
                eprintln!("klocc-gui: root entity missing: {error:#}");
                std::process::exit(1);
            }
        };
        cx.observe_keystrokes(move |event, _, cx| {
            let keystroke = event.keystroke.clone();
            view.update(cx, |view, cx| view.handle_keystroke(&keystroke, cx));
        })
        .detach();
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}

fn window_options(cx: &mut App) -> WindowOptions {
    let width = env::var("KLOCC_GUI_WINDOW_WIDTH")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    let height = env::var("KLOCC_GUI_WINDOW_HEIGHT")
        .ok()
        .and_then(|value| value.parse::<f32>().ok());
    match (width, height) {
        (Some(width), Some(height)) => WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(width), px(height)),
                cx,
            ))),
            ..Default::default()
        },
        _ => WindowOptions::default(),
    }
}

struct Viewer {
    path_text: String,
    artifact: Option<Rc<Artifact>>,
    error: Option<String>,
    metric: MetricMode,
    filter: FilterMode,
    color: ColorMode,
    hierarchy: HierarchyMode,
    completeness: CompletenessMode,
    root: Option<i64>,
    expanded_tiny_group: Option<TinyGroupSelection>,
    history: Vec<Option<i64>>,
    hovered: Option<HitTarget>,
    last_mouse: Option<Point<Pixels>>,
    layout_cache: Rc<RefCell<HashMap<ViewKey, Vec<RectNode>>>>,
    prewarming_layouts: Rc<RefCell<HashSet<ViewKey>>>,
    label_cache: Rc<RefCell<HashMap<LabelFitKey, Option<CachedLabel>>>>,
    last_rects: Rc<RefCell<Vec<RectNode>>>,
    layout_ms: Rc<Cell<f64>>,
    paint_ms: Rc<Cell<f64>>,
    response_ms: Rc<Cell<f64>>,
    response_before_layout_ms: Rc<Cell<f64>>,
    response_layout_pass_ms: Rc<Cell<f64>>,
    pending_interaction: Rc<Cell<Option<Instant>>>,
    _tasks: Vec<Task<()>>,
}

#[derive(Clone)]
struct BreadcrumbItem {
    label: String,
    root: Option<i64>,
    current: bool,
}

#[derive(Default)]
struct PaintProfile {
    enabled: bool,
    fill_ms: f64,
    labels_ms: f64,
    fit_ms: f64,
    value_ms: f64,
    text_ms: f64,
    shape_ms: f64,
    shape_count: usize,
    labels_considered: usize,
    labels_painted: usize,
    labels_skipped: usize,
    labels_truncated: usize,
    label_cache_hits: usize,
    label_cache_misses: usize,
}

#[derive(Clone, Copy)]
struct LabelTextStyle {
    font_size: Pixels,
    weight: FontWeight,
    color: gpui::Hsla,
}

struct LabelPaintState<'a> {
    cache: &'a mut HashMap<LabelFitKey, Option<CachedLabel>>,
    profile: &'a mut PaintProfile,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LabelFitKey {
    source_id: Option<i64>,
    label: String,
    value: String,
    color: u32,
    width: u32,
    height: u32,
}

struct CachedLabel {
    name_line: gpui::ShapedLine,
    value_line: gpui::ShapedLine,
    truncated: bool,
}

struct PrewarmCaches {
    layout: Rc<RefCell<HashMap<ViewKey, Vec<RectNode>>>>,
    label: Rc<RefCell<HashMap<LabelFitKey, Option<CachedLabel>>>>,
    prewarming: Rc<RefCell<HashSet<ViewKey>>>,
}

impl Viewer {
    fn new(initial_path: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path_text = initial_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        let mut this = Self {
            path_text,
            artifact: None,
            error: None,
            metric: MetricMode::Code,
            filter: FilterMode::All,
            color: ColorMode::SourceKind,
            hierarchy: HierarchyMode::SourceKind,
            completeness: CompletenessMode::Counted,
            root: None,
            expanded_tiny_group: None,
            history: Vec::new(),
            hovered: None,
            last_mouse: None,
            layout_cache: Rc::new(RefCell::new(HashMap::new())),
            prewarming_layouts: Rc::new(RefCell::new(HashSet::new())),
            label_cache: Rc::new(RefCell::new(HashMap::new())),
            last_rects: Rc::new(RefCell::new(Vec::new())),
            layout_ms: Rc::new(Cell::new(0.0)),
            paint_ms: Rc::new(Cell::new(0.0)),
            response_ms: Rc::new(Cell::new(0.0)),
            response_before_layout_ms: Rc::new(Cell::new(0.0)),
            response_layout_pass_ms: Rc::new(Cell::new(0.0)),
            pending_interaction: Rc::new(Cell::new(None)),
            _tasks: Vec::new(),
        };
        if let Some(path) = initial_path {
            this.open_path(path, cx);
        }
        if env::var_os("KLOCC_GUI_INTERNAL_PERF").is_some() {
            this.start_internal_perf_driver(window, cx);
        }
        this
    }

    fn handle_keystroke(&mut self, keystroke: &gpui::Keystroke, cx: &mut Context<Self>) {
        if self.artifact.is_some() {
            if !keystroke.modifiers.modified() {
                match keystroke.key.as_str() {
                    "m" => self.cycle_metric(cx),
                    "f" => self.cycle_filter(cx),
                    "c" => self.cycle_color(cx),
                    "g" => self.cycle_hierarchy(cx),
                    "h" => self.cycle_completeness(cx),
                    _ => {}
                }
            }
            return;
        }
        match keystroke.key.as_str() {
            "backspace" => {
                self.path_text.pop();
                cx.notify();
            }
            "enter" => self.open_current_path(cx),
            _ if !keystroke.modifiers.control && !keystroke.modifiers.platform => {
                if let Some(ch) = &keystroke.key_char
                    && !ch.chars().any(|ch| ch.is_control())
                {
                    self.path_text.push_str(ch);
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn open_current_path(&mut self, cx: &mut Context<Self>) {
        self.open_path(PathBuf::from(self.path_text.trim()), cx);
    }

    fn open_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        match Artifact::load(&path) {
            Ok(artifact) => {
                eprintln!(
                    "klocc-gui: loaded {} sources from {} in {:.2}ms",
                    artifact.sources.len(),
                    path.display(),
                    artifact.loaded_ms
                );
                self.path_text = path.display().to_string();
                self.artifact = Some(Rc::new(artifact));
                self.error = None;
                self.root = None;
                self.expanded_tiny_group = None;
                self.hovered = None;
                self.history.clear();
                self.layout_cache.borrow_mut().clear();
                self.prewarming_layouts.borrow_mut().clear();
                self.label_cache.borrow_mut().clear();
                self.last_rects.borrow_mut().clear();
                self.mark_interaction();
            }
            Err(error) => {
                self.error = Some(error.to_string());
            }
        }
        cx.notify();
    }

    fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(SharedString::from("Open scan artifact")),
        });
        cx.spawn_in(window, async move |this, cx| {
            let selected = rx.await??;
            if let Some(path) = selected.and_then(|mut paths| paths.pop()) {
                this.update(cx, |this, cx| this.open_path(path, cx))?;
            }
            anyhow::Ok(())
        })
        .detach();
    }

    fn cycle_metric(&mut self, cx: &mut Context<Self>) {
        self.metric = cycle(self.metric, &MetricMode::ALL);
        self.record_switch(cx);
    }

    fn cycle_filter(&mut self, cx: &mut Context<Self>) {
        self.filter = cycle(self.filter, &FilterMode::ALL);
        self.record_switch(cx);
    }

    fn cycle_color(&mut self, cx: &mut Context<Self>) {
        self.color = cycle(self.color, &ColorMode::ALL);
        self.record_switch(cx);
    }

    fn cycle_hierarchy(&mut self, cx: &mut Context<Self>) {
        self.hierarchy = cycle(self.hierarchy, &HierarchyMode::ALL);
        self.root = None;
        self.expanded_tiny_group = None;
        self.history.clear();
        self.record_switch(cx);
    }

    fn navigate_to_breadcrumb(&mut self, root: Option<i64>, cx: &mut Context<Self>) {
        if let Some(index) = self.history.iter().position(|item| *item == root) {
            self.history.truncate(index);
        } else if root == self.root {
            return;
        } else {
            self.history.clear();
        }
        self.root = root;
        self.expanded_tiny_group = None;
        self.hovered = None;
        self.mark_interaction();
        cx.notify();
    }

    fn cycle_completeness(&mut self, cx: &mut Context<Self>) {
        self.completeness = cycle(self.completeness, &CompletenessMode::ALL);
        self.record_switch(cx);
    }

    fn record_switch(&mut self, cx: &mut Context<Self>) {
        self.expanded_tiny_group = None;
        self.hovered = None;
        self.mark_interaction();
        cx.notify();
    }

    fn go_back(&mut self, cx: &mut Context<Self>) {
        if self.expanded_tiny_group.take().is_some() {
            self.hovered = None;
            self.mark_interaction();
            cx.notify();
            return;
        }
        if let Some(root) = self.history.pop() {
            self.root = root;
        }
        self.hovered = None;
        self.mark_interaction();
        cx.notify();
    }

    fn drill_into(&mut self, source_id: i64, cx: &mut Context<Self>) {
        let Some(artifact) = &self.artifact else { return };
        let key = ViewKey {
            root: Some(source_id),
            metric: self.metric,
            filter: self.filter,
            color: self.color,
            hierarchy: self.hierarchy,
            completeness: self.completeness,
            expanded_tiny_group: None,
            width: 0,
            height: 0,
        };
        if !has_visible_children(artifact, source_id, &key) {
            return;
        }
        self.history.push(self.root);
        self.root = Some(source_id);
        self.expanded_tiny_group = None;
        self.hovered = None;
        self.mark_interaction();
        cx.notify();
    }

    fn expand_tiny_group(&mut self, selection: TinyGroupSelection, cx: &mut Context<Self>) {
        self.expanded_tiny_group = Some(selection);
        self.hovered = None;
        self.mark_interaction();
        cx.notify();
    }

    fn activate_hit(&mut self, hit: HitTarget, cx: &mut Context<Self>) {
        match hit {
            HitTarget::Source(source_id) => self.drill_into(source_id, cx),
            HitTarget::TinyGroup(selection) => self.expand_tiny_group(selection, cx),
        }
    }

    fn update_hover(&mut self, point: Point<Pixels>, hovered: Option<HitTarget>, cx: &mut Context<Self>) {
        self.last_mouse = Some(point);
        if hovered != self.hovered {
            self.hovered = hovered;
            self.mark_interaction();
            cx.notify();
        }
    }

    fn mark_interaction(&self) {
        self.pending_interaction.set(Some(Instant::now()));
    }

    fn start_internal_perf_driver(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let quit_when_done = env::var_os("KLOCC_GUI_INTERNAL_PERF_QUIT").is_some();
        let initial_delay = env_duration_ms("KLOCC_GUI_INTERNAL_PERF_INITIAL_MS", 3000);
        let settle_delay = env_duration_ms("KLOCC_GUI_INTERNAL_PERF_SETTLE_MS", 500);
        let state_hold = env_duration_ms("KLOCC_GUI_INTERNAL_PERF_HOLD_MS", 250);
        let root_return_delay = env_duration_ms("KLOCC_GUI_INTERNAL_PERF_ROOT_RETURN_MS", 700);
        let task = cx.spawn_in(window, async move |this, cx| {
            cx.background_executor().timer(initial_delay).await;

            eprintln!("klocc-gui-perf: state 01-current begin");
            let _ = this.update_in(cx, |this, window, cx| {
                if this.hover_largest_source_for_perf(cx) {
                    window.refresh();
                }
            });
            cx.background_executor().timer(state_hold).await;
            eprintln!("klocc-gui-perf: state 01-current end");

            let root_tiny = this
                .update_in(cx, |this, _, _| this.largest_tiny_hit_for_perf())
                .ok()
                .flatten();

            eprintln!("klocc-gui-perf: state 02-largest-top-left begin");
            let _ = this.update_in(cx, |this, window, cx| {
                if this.drill_largest_drillable_for_perf(cx) {
                    window.refresh();
                }
            });
            cx.background_executor().timer(settle_delay).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.hover_largest_source_for_perf(cx) {
                    window.refresh();
                }
            });
            cx.background_executor().timer(state_hold).await;
            eprintln!("klocc-gui-perf: state 02-largest-top-left end");

            eprintln!("klocc-gui-perf: state 03-third-layer begin");
            let _ = this.update_in(cx, |this, window, cx| {
                if this.drill_largest_drillable_for_perf(cx) {
                    window.refresh();
                }
            });
            cx.background_executor().timer(settle_delay).await;
            let _ = this.update_in(cx, |this, window, cx| {
                if this.hover_largest_source_for_perf(cx) {
                    window.refresh();
                }
            });
            cx.background_executor().timer(state_hold).await;
            eprintln!("klocc-gui-perf: state 03-third-layer end");

            let _ = this.update_in(cx, |this, window, cx| {
                this.navigate_to_breadcrumb(None, cx);
                window.refresh();
            });
            cx.background_executor().timer(root_return_delay).await;

            if let Some(root_tiny) = root_tiny {
                eprintln!("klocc-gui-perf: state 04-gray-tiny begin");
                let _ = this.update_in(cx, |this, window, cx| {
                    this.activate_hit(root_tiny, cx);
                    window.refresh();
                });
                cx.background_executor().timer(settle_delay).await;
                let _ = this.update_in(cx, |this, window, cx| {
                    if this.hover_largest_source_for_perf(cx) {
                        window.refresh();
                    }
                });
                cx.background_executor().timer(state_hold).await;
                eprintln!("klocc-gui-perf: state 04-gray-tiny end");
            }

            if quit_when_done {
                let _ = cx.update(|_, cx| cx.quit());
            }
        });
        self._tasks.push(task);
    }

    fn hover_largest_source_for_perf(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(hit) = self.largest_source_hit_for_perf(false) else {
            return false;
        };
        if self.hovered.as_ref() == Some(&hit) {
            return true;
        }
        self.hovered = Some(hit);
        self.mark_interaction();
        cx.notify();
        true
    }

    fn drill_largest_drillable_for_perf(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(HitTarget::Source(source_id)) = self.largest_source_hit_for_perf(true) else {
            return false;
        };
        self.drill_into(source_id, cx);
        true
    }

    fn largest_source_hit_for_perf(&self, drillable_only: bool) -> Option<HitTarget> {
        let artifact = self.artifact.as_ref();
        self.last_rects
            .borrow()
            .iter()
            .filter(|rect| rect.source_id.is_some())
            .filter(|rect| {
                !drillable_only
                    || rect
                        .source_id
                        .is_some_and(|source_id| artifact.is_some_and(|artifact| artifact.has_children(source_id)))
            })
            .max_by(|left, right| rect_area(left).total_cmp(&rect_area(right)))
            .and_then(rect_hit_target)
    }

    fn largest_tiny_hit_for_perf(&self) -> Option<HitTarget> {
        self.last_rects
            .borrow()
            .iter()
            .filter(|rect| rect.tiny_group_key.is_some())
            .max_by(|left, right| rect_area(left).total_cmp(&rect_area(right)))
            .and_then(rect_hit_target)
    }

    fn hovered_node(&self) -> Option<&SourceNode> {
        let artifact = self.artifact.as_ref()?;
        let HitTarget::Source(id) = self.hovered.as_ref()? else {
            return None;
        };
        artifact.source(*id)
    }

    fn current_node(&self) -> Option<&SourceNode> {
        let artifact = self.artifact.as_ref()?;
        artifact.source(self.root?)
    }

    fn root_label(&self) -> String {
        if let Some(selection) = &self.expanded_tiny_group {
            return format!("tiny {}", human_group(&selection.group_key));
        }
        let Some(root) = self.root else {
            return self.project_label();
        };
        self.artifact
            .as_ref()
            .and_then(|artifact| artifact.source(root))
            .map(|node| human_label(&node.display_name()))
            .unwrap_or_else(|| root.to_string())
    }

    fn project_label(&self) -> String {
        self.artifact
            .as_ref()
            .and_then(|artifact| {
                artifact
                    .sources
                    .iter()
                    .find(|source| source.ecosystem == "cargo-workspace")
                    .or_else(|| artifact.sources.first())
            })
            .map(|source| human_label(&source.display_name()))
            .unwrap_or_else(|| "root".to_string())
    }

    fn breadcrumb_items(&self) -> Vec<BreadcrumbItem> {
        let mut items = vec![BreadcrumbItem {
            label: self.project_label(),
            root: None,
            current: self.root.is_none() && self.expanded_tiny_group.is_none(),
        }];
        for root in self.history.iter().filter_map(|root| *root) {
            if items.iter().any(|item| item.root == Some(root)) {
                continue;
            }
            items.push(BreadcrumbItem {
                label: self.source_label(root),
                root: Some(root),
                current: false,
            });
        }
        if let Some(root) = self.root
            && !items.iter().any(|item| item.root == Some(root))
        {
            items.push(BreadcrumbItem {
                label: self.source_label(root),
                root: Some(root),
                current: self.expanded_tiny_group.is_none(),
            });
        }
        if let Some(selection) = &self.expanded_tiny_group {
            items.push(BreadcrumbItem {
                label: format!("tiny {}", human_group(&selection.group_key)),
                root: self.root,
                current: true,
            });
        }
        if let Some(last) = items.last_mut() {
            last.current = true;
        }
        items
    }

    fn source_label(&self, source_id: i64) -> String {
        self.artifact
            .as_ref()
            .and_then(|artifact| artifact.source(source_id))
            .map(|node| human_label(&node.display_name()))
            .unwrap_or_else(|| source_id.to_string())
    }

    fn source_count_summary(&self) -> String {
        let Some(artifact) = self.artifact.as_ref() else {
            return "0/0".to_string();
        };
        let total = artifact.sources.len();
        let rendered_count = rendered_source_count(&self.last_rects.borrow());
        let shown = if self.expanded_tiny_group.is_some() && rendered_count > 0 {
            rendered_count
        } else {
            let key = ViewKey {
                root: self.root,
                metric: self.metric,
                filter: self.filter,
                color: self.color,
                hierarchy: self.hierarchy,
                completeness: self.completeness,
                expanded_tiny_group: self.expanded_tiny_group.clone(),
                width: 0,
                height: 0,
            };
            source_count_for_view(artifact, &key)
        };
        format!("{shown}/{total}")
    }
}

fn rendered_source_count(rects: &[RectNode]) -> usize {
    rects
        .iter()
        .map(|rect| if rect.source_id.is_some() { 1 } else { rect.tiny_count })
        .sum()
}

fn rect_area(rect: &RectNode) -> f32 {
    rect.bounds.size.width.as_f32() * rect.bounds.size.height.as_f32()
}

fn env_duration_ms(name: &str, default_ms: u64) -> Duration {
    env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or_else(|| Duration::from_millis(default_ms))
}

fn schedule_tiny_layout_prewarm(
    bounds: Bounds<Pixels>,
    rects: &[RectNode],
    base_key: &ViewKey,
    artifact: Rc<Artifact>,
    caches: PrewarmCaches,
    window: &mut Window,
) {
    let mut keys = Vec::new();
    {
        let cache = caches.layout.borrow();
        let mut prewarming = caches.prewarming.borrow_mut();
        for rect in rects {
            let Some(group_key) = rect.tiny_group_key.as_ref() else {
                continue;
            };
            if rect.tiny_source_ids.is_empty() {
                continue;
            }
            let mut key = base_key.clone();
            key.expanded_tiny_group = Some(TinyGroupSelection {
                group_key: group_key.clone(),
                source_ids: rect.tiny_source_ids.clone(),
            });
            if !cache.contains_key(&key) && prewarming.insert(key.clone()) {
                keys.push(key);
            }
        }
    }
    if keys.is_empty() {
        return;
    }

    window.on_next_frame(move |window, _| {
        for key in keys {
            let expanded_rects = build_layout(&artifact, &key, bounds);
            caches.layout.borrow_mut().insert(key.clone(), expanded_rects.clone());
            prewarm_rect_labels(&expanded_rects, caches.label.clone(), window);
            caches.prewarming.borrow_mut().remove(&key);
        }
    });
}

fn prewarm_rect_labels(
    rects: &[RectNode],
    label_cache: Rc<RefCell<HashMap<LabelFitKey, Option<CachedLabel>>>>,
    window: &mut Window,
) {
    let mut label_cache = label_cache.borrow_mut();
    let mut profile = PaintProfile::default();
    for rect in rects.iter().filter(|rect| rect_display_text(rect).is_some()).take(220) {
        let Some(display_text) = rect_display_text(rect) else {
            continue;
        };
        let Some((key, available_width)) = label_fit_key(rect, &display_text.name, &display_text.value) else {
            continue;
        };
        if label_cache.contains_key(&key) {
            continue;
        }
        let cached = fit_cached_label(
            &display_text.name,
            &display_text.value,
            rect.color,
            available_width,
            window,
            &mut profile,
        );
        label_cache.insert(key, cached);
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.artifact.is_none() {
            return self.render_open(window, cx).into_any_element();
        }

        let hovered = self.hovered_node().cloned();
        let focused_source_title = if hovered.is_some() {
            "hovered source"
        } else {
            "current source"
        };
        let focused_source = hovered.or_else(|| self.current_node().cloned());
        let health = self
            .artifact
            .as_ref()
            .map(|artifact| artifact.health.clone())
            .unwrap_or_default();
        let path = self
            .artifact
            .as_ref()
            .map(|artifact| artifact.path.display().to_string())
            .unwrap_or_default();
        let root_label = self.root_label();
        let source_count_summary = self.source_count_summary();

        div()
            .size_full()
            .bg(rgb(theme::BASE))
            .text_color(rgb(theme::TEXT))
            .flex()
            .flex_col()
            .child(self.toolbar(cx))
            .child(
                div()
                    .flex()
                    .size_full()
                    .child(self.side_panel(
                        focused_source,
                        focused_source_title,
                        &health,
                        &path,
                        &root_label,
                        &source_count_summary,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .size_full()
                            .child(self.breadcrumb_bar(cx))
                            .child(self.treemap_canvas(cx))
                            .child(self.color_legend()),
                    ),
            )
            .into_any_element()
    }
}

impl Viewer {
    fn render_open(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let error = self.error.clone();
        div()
            .size_full()
            .bg(rgb(theme::BASE))
            .text_color(rgb(theme::TEXT))
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .w(px(620.))
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(theme::SURFACE1))
                    .bg(rgb(theme::MANTLE))
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(div().text_size(px(20.)).child("Open artifact"))
                    .child(
                        div()
                            .h(px(42.))
                            .w_full()
                            .px_3()
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(theme::SURFACE2))
                            .bg(rgb(theme::CRUST))
                            .child(if self.path_text.is_empty() {
                                "/tmp/klocc-self.sqlite".to_string()
                            } else {
                                self.path_text.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(button("open", cx, |this, cx| this.open_current_path(cx)))
                            .child(button_window("choose file", cx, |this, window, cx| {
                                this.open_picker(window, cx)
                            })),
                    )
                    .when_some(error, |this, error| {
                        this.child(
                            div()
                                .text_color(rgb(theme::RED))
                                .border_1()
                                .border_color(rgb(theme::RED))
                                .rounded_md()
                                .p_2()
                                .child(error),
                        )
                    }),
            )
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(46.))
            .px_4()
            .gap_2()
            .flex()
            .items_center()
            .border_b_1()
            .border_color(rgb(theme::SURFACE0))
            .bg(rgb(theme::MANTLE))
            .child(selector(
                format!("metric {}", self.metric.label()),
                cx,
                Self::cycle_metric,
            ))
            .child(selector(
                format!("filter {}", self.filter.label()),
                cx,
                Self::cycle_filter,
            ))
            .child(selector(format!("color {}", self.color.label()), cx, Self::cycle_color))
            .child(selector(
                format!("hierarchy {}", self.hierarchy.label()),
                cx,
                Self::cycle_hierarchy,
            ))
            .child(selector(
                format!("health {}", self.completeness.label()),
                cx,
                Self::cycle_completeness,
            ))
            .child(div().flex_1())
            .child(div().text_color(rgb(theme::SUBTEXT0)).child(format!(
                "layout {:.2}ms paint {:.2}ms response {:.2}ms",
                self.layout_ms.get(),
                self.paint_ms.get(),
                self.response_ms.get()
            )))
    }

    fn breadcrumb_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let items = self.breadcrumb_items();
        let mut bar = div()
            .h(px(28.0))
            .px_8()
            .gap_2()
            .flex()
            .items_center()
            .overflow_hidden()
            .text_sm();
        for (index, item) in items.into_iter().enumerate() {
            if index > 0 {
                bar = bar.child(div().text_color(rgb(theme::SURFACE2)).child("->"));
            }
            bar = if item.current {
                bar.child(
                    div()
                        .max_w(px(280.0))
                        .overflow_hidden()
                        .text_color(rgb(theme::TEXT))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(item.label),
                )
            } else {
                let root = item.root;
                bar.child(
                    div()
                        .max_w(px(280.0))
                        .overflow_hidden()
                        .text_color(rgb(theme::BLUE))
                        .hover(|style| style.text_color(rgb(theme::LAVENDER)).cursor_pointer())
                        .child(item.label)
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.navigate_to_breadcrumb(root, cx)),
                        ),
                )
            };
        }
        bar
    }

    fn side_panel(
        &self,
        focused_source: Option<SourceNode>,
        focused_source_title: &str,
        health: &BTreeMap<String, i64>,
        path: &str,
        root_label: &str,
        source_count_summary: &str,
    ) -> impl IntoElement {
        let mut panel = div()
            .w(px(320.))
            .h_full()
            .p_3()
            .gap_1()
            .flex()
            .flex_col()
            .text_sm()
            .overflow_hidden()
            .border_r_1()
            .border_color(rgb(theme::SURFACE0))
            .bg(rgb(theme::MANTLE));

        panel = panel
            .child(section_title("overview"))
            .child(detail("artifact", path))
            .child(detail("root", root_label))
            .child(detail("sources", source_count_summary))
            .child(detail("unknown", metric_value(health, "unknown_derivation_sources")))
            .child(detail(
                "generated",
                metric_value(health, "generated_derivation_outputs"),
            ));

        if let Some(node) = focused_source {
            panel = panel
                .child(div().mt_3().child(section_title(focused_source_title.to_string())))
                .child(detail("name", human_label(&node.display_name())))
                .child(detail("kind", node.source_kind))
                .child(detail("ecosystem", node.ecosystem))
                .child(detail("status", node.realization_status))
                .child(detail("confidence", node.confidence))
                .child(detail(
                    "layer",
                    if node.runtime_linked {
                        "runtime-linked"
                    } else {
                        "build-time-only"
                    },
                ))
                .child(detail("own code", format_i64(node.own_code_loc)))
                .child(detail("total", format_i64(node.total_code_loc)))
                .child(detail("unique", format_i64(node.unique_transitive_code_loc)))
                .child(detail("shared", format_i64(node.shared_transitive_code_loc)))
                .child(detail("deps", format_i64(node.reachable_source_count)));
            if let Some(path) = node.source_path {
                panel = panel.child(detail("path", path));
            }
            if let Some(derivation) = node.derivations.first() {
                panel = panel.child(detail("drv", derivation.clone()));
            }
        }

        panel
    }

    fn color_legend(&self) -> impl IntoElement {
        let items: Vec<(&'static str, u32)> = match self.color {
            ColorMode::SourceKind => vec![
                ("fixed output", theme::BLUE),
                ("drv source", theme::PEACH),
                ("cargo", theme::MAUVE),
                ("generated", theme::YELLOW),
                ("source output", theme::TEAL),
            ],
            ColorMode::Ecosystem => vec![
                ("nix", theme::BLUE),
                ("cargo", theme::MAUVE),
                ("workspace", theme::GREEN),
            ],
            ColorMode::Layer => vec![("runtime", theme::TEAL), ("build", theme::MAUVE)],
            ColorMode::Health => vec![
                ("counted", theme::GREEN),
                ("generated", theme::YELLOW),
                ("missing", theme::RED),
            ],
        };
        let mut legend = div()
            .h(px(30.0))
            .px_8()
            .pb_2()
            .gap_3()
            .flex()
            .items_center()
            .overflow_hidden()
            .child(div().text_xs().text_color(rgb(theme::BLUE)).child("color"));
        for (label, color) in items {
            legend = legend.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(10.0)).h(px(10.0)).rounded_sm().bg(rgb(color)))
                    .child(div().text_xs().text_color(rgb(theme::TEXT)).child(label)),
            );
        }
        legend
    }

    fn treemap_canvas(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let artifact_for_layout = self.artifact.clone();
        let artifact_for_prewarm = self.artifact.clone();
        let layout_cache_for_layout = self.layout_cache.clone();
        let layout_cache_for_prewarm = self.layout_cache.clone();
        let prewarming_layouts = self.prewarming_layouts.clone();
        let label_cache = self.label_cache.clone();
        let last_rects = self.last_rects.clone();
        let layout_ms_for_layout = self.layout_ms.clone();
        let layout_ms_for_paint = self.layout_ms.clone();
        let paint_ms = self.paint_ms.clone();
        let response_ms = self.response_ms.clone();
        let response_before_layout_ms_for_layout = self.response_before_layout_ms.clone();
        let response_before_layout_ms_for_paint = self.response_before_layout_ms.clone();
        let response_layout_pass_ms_for_layout = self.response_layout_pass_ms.clone();
        let response_layout_pass_ms_for_paint = self.response_layout_pass_ms.clone();
        let pending_interaction_for_layout = self.pending_interaction.clone();
        let pending_interaction = self.pending_interaction.clone();
        let profile_paint = env::var_os("KLOCC_GUI_PROFILE").is_some();
        let internal_perf = env::var_os("KLOCC_GUI_INTERNAL_PERF").is_some();
        let state = (
            self.root,
            self.metric,
            self.filter,
            self.color,
            self.hierarchy,
            self.completeness,
            self.expanded_tiny_group.clone(),
            self.hovered.clone(),
        );
        let layout_state = state.clone();
        let paint_state = state;
        div().w_full().flex_1().px_8().pt_1().pb_2().child(
            div()
                .size_full()
                .rounded_lg()
                .border_1()
                .border_color(rgb(theme::SURFACE0))
                .bg(rgb(theme::BASE))
                .child(
                    canvas(
                        move |bounds, _, _| {
                            let Some(artifact) = artifact_for_layout.as_ref() else {
                                return Vec::new();
                            };
                            let key = ViewKey {
                                root: layout_state.0,
                                metric: layout_state.1,
                                filter: layout_state.2,
                                color: layout_state.3,
                                hierarchy: layout_state.4,
                                completeness: layout_state.5,
                                expanded_tiny_group: layout_state.6.clone(),
                                width: bounds.size.width.as_f32().round() as u32,
                                height: bounds.size.height.as_f32().round() as u32,
                            };
                            if let Some(interaction_started) = pending_interaction_for_layout.get() {
                                response_before_layout_ms_for_layout.set(
                                    Instant::now()
                                        .duration_since(interaction_started)
                                        .as_secs_f64()
                                        * 1000.0,
                                );
                            }
                            if let Some(rects) = layout_cache_for_layout.borrow().get(&key) {
                                layout_ms_for_layout.set(0.0);
                                response_layout_pass_ms_for_layout.set(0.0);
                                last_rects.borrow_mut().clone_from(rects);
                                return rects.clone();
                            }
                            let started = Instant::now();
                            let rects = build_layout(artifact, &key, bounds);
                            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                            layout_ms_for_layout.set(elapsed_ms);
                            response_layout_pass_ms_for_layout.set(elapsed_ms);
                            eprintln!(
                                "klocc-gui: layout {:?}/{:?}/{:?}/{:?}/{:?} root {:?}: {} rects in {:.3}ms",
                                layout_state.1,
                                layout_state.2,
                                layout_state.3,
                                layout_state.4,
                                layout_state.5,
                                layout_state.0,
                                rects.len(),
                                elapsed_ms
                            );
                            eprintln!("klocc-gui: display summary {}", layout_debug_summary(&rects));
                            eprintln!("klocc-gui: display size histogram {}", layout_size_histogram(&rects));
                            eprintln!("klocc-gui: display ascii map\n{}", layout_ascii_map(&rects, 96, 36));
                            eprintln!("klocc-gui: display rects\n{}", layout_rect_manifest(&rects));
                            layout_cache_for_layout.borrow_mut().insert(key, rects.clone());
                            last_rects.borrow_mut().clone_from(&rects);
                            rects
                        },
                        move |bounds, rects, window, app| {
                            let started = Instant::now();
                            let mut painted_labels = 0usize;
                            let label_cache_for_prewarm = label_cache.clone();
                            let base_key = ViewKey {
                                root: paint_state.0,
                                metric: paint_state.1,
                                filter: paint_state.2,
                                color: paint_state.3,
                                hierarchy: paint_state.4,
                                completeness: paint_state.5,
                                expanded_tiny_group: None,
                                width: bounds.size.width.as_f32().round() as u32,
                                height: bounds.size.height.as_f32().round() as u32,
                            };
                            let mut label_cache = label_cache.borrow_mut();
                            let mut profile = PaintProfile {
                                enabled: profile_paint,
                                ..Default::default()
                            };
                            for rect in &rects {
                                let mut color = rgb(rect.color);
                                let hovered = rect_hit_target(rect).as_ref() == paint_state.7.as_ref();
                                if hovered {
                                    color = color.blend(rgba(theme::HOVER_OVERLAY));
                                }
                                let fill_started = profile.enabled.then(Instant::now);
                                window.paint_quad(quad(
                                    rect.bounds,
                                    px(3.),
                                    color,
                                    if hovered { px(4.) } else { px(1.) },
                                    if hovered { rgb(theme::YELLOW) } else { rgb(theme::BASE) },
                                    Default::default(),
                                ));
                                if let Some(started) = fill_started {
                                    profile.fill_ms += elapsed_ms(started);
                                }
                                if painted_labels < 220 && let Some(display_text) = rect_display_text(rect) {
                                    let mut label_paint = LabelPaintState {
                                        cache: &mut label_cache,
                                        profile: &mut profile,
                                    };
                                    if paint_rect_label(
                                        rect,
                                        &display_text.name,
                                        &display_text.value,
                                        window,
                                        app,
                                        &mut label_paint,
                                    ) {
                                        painted_labels += 1;
                                    }
                                }
                            }
                            let quality = layout_quality(&rects, painted_labels);
                            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                            paint_ms.set(elapsed_ms);
                            if let Some(interaction_started) = pending_interaction.take() {
                                let total_response_ms = interaction_started.elapsed().as_secs_f64() * 1000.0;
                                let before_paint_ms = started
                                    .duration_since(interaction_started)
                                    .as_secs_f64()
                                    * 1000.0;
                                let current_layout_ms = layout_ms_for_paint.get();
                                let before_layout_ms = response_before_layout_ms_for_paint.get();
                                let layout_pass_ms = response_layout_pass_ms_for_paint.get();
                                let between_layout_paint_ms =
                                    (before_paint_ms - before_layout_ms - layout_pass_ms).max(0.0);
                                response_ms.set(total_response_ms);
                                eprintln!(
                                    "klocc-gui: response before_layout={:.3}ms layout={:.3}ms between_layout_paint={:.3}ms before_paint={:.3}ms paint={:.3}ms total={:.3}ms",
                                    before_layout_ms,
                                    current_layout_ms,
                                    between_layout_paint_ms,
                                    before_paint_ms,
                                    elapsed_ms,
                                    total_response_ms
                                );
                            }
                            eprintln!(
                                "klocc-gui: paint quality rects {} labels {} slivers {} tiny {} aspect {:.2}-{:.2} in {:.3}ms",
                                quality.rects,
                                quality.labels,
                                quality.slivers,
                                quality.tiny,
                                quality.min_aspect,
                                quality.max_aspect,
                                elapsed_ms
                            );
                            if profile.enabled {
                                eprintln!(
                                    "klocc-gui: paint profile rects={} candidates={} painted={} skipped={} truncated={} cache_hits={} cache_misses={} fill={:.3}ms labels={:.3}ms fit={:.3}ms value={:.3}ms text={:.3}ms shape={:.3}ms shapes={}",
                                    rects.len(),
                                    profile.labels_considered,
                                    profile.labels_painted,
                                    profile.labels_skipped,
                                    profile.labels_truncated,
                                    profile.label_cache_hits,
                                    profile.label_cache_misses,
                                    profile.fill_ms,
                                    profile.labels_ms,
                                    profile.fit_ms,
                                    profile.value_ms,
                                    profile.text_ms,
                                    profile.shape_ms,
                                    profile.shape_count
                                );
                            }
                            if paint_state.6.is_none()
                                && let Some(artifact) = artifact_for_prewarm.as_ref()
                            {
                                schedule_tiny_layout_prewarm(
                                    bounds,
                                    &rects,
                                    &base_key,
                                    artifact.clone(),
                                    PrewarmCaches {
                                        layout: layout_cache_for_prewarm.clone(),
                                        label: label_cache_for_prewarm,
                                        prewarming: prewarming_layouts.clone(),
                                    },
                                    window,
                                );
                            }
                            if internal_perf {
                                window.request_animation_frame();
                            }
                        },
                    )
                    .size_full(),
                )
                .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                    let hovered = {
                        let rects = this.last_rects.borrow();
                        hit_test(ev.position, &rects)
                    };
                    this.update_hover(ev.position, hovered, cx);
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        this.last_mouse = Some(ev.position);
                        let hit = {
                            let rects = this.last_rects.borrow();
                            hit_test(ev.position, &rects)
                        };
                        if let Some(hit) = hit {
                            this.activate_hit(hit, cx);
                        }
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        this.last_mouse = Some(ev.position);
                        this.go_back(cx);
                    }),
                ),
        )
    }
}

fn selector(
    label: String,
    cx: &mut Context<Viewer>,
    on_click: impl Fn(&mut Viewer, &mut Context<Viewer>) + 'static,
) -> impl IntoElement {
    div()
        .id(label.clone())
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme::SURFACE1))
        .bg(rgb(theme::CRUST))
        .hover(|style| style.bg(rgb(theme::SURFACE0)).cursor_pointer())
        .child(label)
        .on_mouse_up(MouseButton::Left, cx.listener(move |this, _, _, cx| on_click(this, cx)))
}

fn button(
    label: &'static str,
    cx: &mut Context<Viewer>,
    on_click: impl Fn(&mut Viewer, &mut Context<Viewer>) + 'static,
) -> impl IntoElement {
    selector(label.to_string(), cx, on_click)
}

fn button_window(
    label: &'static str,
    cx: &mut Context<Viewer>,
    on_click: impl Fn(&mut Viewer, &mut Window, &mut Context<Viewer>) + 'static,
) -> impl IntoElement {
    div()
        .id(label)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(rgb(theme::BLUE))
        .bg(rgb(theme::CRUST))
        .hover(|style| style.bg(rgb(theme::SURFACE0)).cursor_pointer())
        .child(label)
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| on_click(this, window, cx)),
        )
}

fn detail(label: impl Into<String>, value: impl Into<String>) -> impl IntoElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(74.0))
                .pt(px(1.0))
                .text_xs()
                .text_color(rgb(theme::SUBTEXT0))
                .child(label.into()),
        )
        .child(
            div()
                .flex_1()
                .text_sm()
                .overflow_hidden()
                .text_color(rgb(theme::TEXT))
                .child(value.into()),
        )
}

fn section_title(label: impl Into<String>) -> impl IntoElement {
    div()
        .text_xs()
        .text_color(rgb(theme::BLUE))
        .font_weight(FontWeight::SEMIBOLD)
        .child(label.into())
}

fn rect_hit_target(rect: &RectNode) -> Option<HitTarget> {
    rect.source_id.map(HitTarget::Source).or_else(|| {
        rect.tiny_group_key.as_ref().map(|group_key| {
            HitTarget::TinyGroup(TinyGroupSelection {
                group_key: group_key.clone(),
                source_ids: rect.tiny_source_ids.clone(),
            })
        })
    })
}

fn paint_rect_label(
    rect: &RectNode,
    name: &str,
    value: &str,
    window: &mut Window,
    cx: &mut App,
    state: &mut LabelPaintState<'_>,
) -> bool {
    let profile = &mut *state.profile;
    profile.labels_considered += 1;
    let label_started = profile.enabled.then(Instant::now);
    let padding = 3.0;
    let Some((key, available_width)) = label_fit_key(rect, name, value) else {
        profile.labels_skipped += 1;
        return false;
    };
    let x = (rect.bounds.left().as_f32() + padding).round();
    let y = (rect.bounds.top().as_f32() + padding).round();
    let name_origin = point(px(x), px(y));
    let value_origin = point(px(x), px(y + 11.0));

    if let Some(cached) = state.cache.get(&key) {
        profile.label_cache_hits += 1;
        let Some(cached) = cached else {
            profile.labels_skipped += 1;
            return false;
        };
        paint_cached_label(cached, name_origin, value_origin, window, cx, profile);
        if let Some(started) = label_started {
            profile.labels_ms += elapsed_ms(started);
        }
        profile.labels_painted += 1;
        return true;
    }
    profile.label_cache_misses += 1;
    let cached = fit_cached_label(name, value, rect.color, available_width, window, profile);
    let Some(cached) = cached else {
        state.cache.insert(key, None);
        profile.labels_skipped += 1;
        return false;
    };
    paint_cached_label(&cached, name_origin, value_origin, window, cx, profile);
    if let Some(started) = label_started {
        profile.labels_ms += elapsed_ms(started);
    }
    if cached.truncated {
        profile.labels_truncated += 1;
    }
    profile.labels_painted += 1;
    state.cache.insert(key, Some(cached));
    true
}

fn label_fit_key(rect: &RectNode, name: &str, value: &str) -> Option<(LabelFitKey, f32)> {
    let padding = 3.0;
    let available_width = rect.bounds.size.width.as_f32() - padding * 2.0;
    if available_width < 28.0 || rect.bounds.size.height.as_f32() < 25.0 {
        return None;
    }
    Some((
        LabelFitKey {
            source_id: rect.source_id,
            label: name.to_string(),
            value: value.to_string(),
            color: rect.color,
            width: available_width.round() as u32,
            height: rect.bounds.size.height.as_f32().round() as u32,
        },
        available_width,
    ))
}

fn fit_cached_label(
    name: &str,
    value: &str,
    color: u32,
    available_width: f32,
    window: &mut Window,
    profile: &mut PaintProfile,
) -> Option<CachedLabel> {
    let text_color = label_color(color);
    let name_style = LabelTextStyle {
        font_size: px(10.0),
        weight: FontWeight::SEMIBOLD,
        color: text_color,
    };
    let value_style = LabelTextStyle {
        font_size: px(9.0),
        weight: FontWeight::MEDIUM,
        color: text_color,
    };
    let fit_started = profile.enabled.then(Instant::now);
    let (_name, name_line, truncated) = fit_text_to_width(name, available_width, name_style, window, profile)?;
    if let Some(started) = fit_started {
        profile.fit_ms += elapsed_ms(started);
    }
    let value_started = profile.enabled.then(Instant::now);
    let value_line = shape_text_line(
        value,
        value_style.font_size,
        value_style.weight,
        value_style.color,
        window,
        profile,
    );
    if let Some(started) = value_started {
        profile.value_ms += elapsed_ms(started);
    }
    if value_line.width().as_f32() > available_width {
        return None;
    }
    Some(CachedLabel {
        name_line,
        value_line,
        truncated,
    })
}

fn paint_cached_label(
    cached: &CachedLabel,
    name_origin: Point<Pixels>,
    value_origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
    profile: &mut PaintProfile,
) {
    let text_started = profile.enabled.then(Instant::now);
    let _ = cached
        .name_line
        .paint(name_origin, px(10.0), TextAlign::Left, None, window, cx);
    let _ = cached
        .value_line
        .paint(value_origin, px(9.0), TextAlign::Left, None, window, cx);
    if let Some(started) = text_started {
        profile.text_ms += elapsed_ms(started);
    }
}

fn fit_text_to_width(
    text: &str,
    width: f32,
    style: LabelTextStyle,
    window: &mut Window,
    profile: &mut PaintProfile,
) -> Option<(String, gpui::ShapedLine, bool)> {
    let full_line = shape_text_line(text, style.font_size, style.weight, style.color, window, profile);
    if full_line.width().as_f32() <= width {
        return Some((text.to_string(), full_line, false));
    }
    let ellipsis_width = shape_text_line("...", style.font_size, style.weight, style.color, window, profile)
        .width()
        .as_f32();
    if ellipsis_width > width {
        return None;
    }
    let prefix_width = (width - ellipsis_width).max(0.0);
    let mut byte_index = full_line.index_for_x(px(prefix_width)).unwrap_or(text.len());
    byte_index = previous_char_boundary(text, byte_index.min(text.len()));
    while byte_index > 0 {
        let candidate = truncated_text(text, byte_index);
        let line = shape_text_line(&candidate, style.font_size, style.weight, style.color, window, profile);
        if line.width().as_f32() <= width {
            return Some((candidate, line, true));
        }
        byte_index = previous_char_boundary(text, byte_index.saturating_sub(1));
    }
    None
}

fn truncated_text(text: &str, byte_index: usize) -> String {
    let mut out = text[..byte_index].to_string();
    out.push_str("...");
    out
}

fn previous_char_boundary(text: &str, mut byte_index: usize) -> usize {
    while byte_index > 0 && !text.is_char_boundary(byte_index) {
        byte_index -= 1;
    }
    byte_index
}

fn label_color(background: u32) -> gpui::Hsla {
    let dark = theme::CRUST;
    let light = theme::TEXT;
    if contrast_ratio(dark, background) >= contrast_ratio(light, background) {
        rgb(dark).into()
    } else {
        rgb(light).into()
    }
}

fn contrast_ratio(left: u32, right: u32) -> f32 {
    let left = relative_luminance(left);
    let right = relative_luminance(right);
    let lighter = left.max(right);
    let darker = left.min(right);
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: u32) -> f32 {
    let r = ((color >> 16) & 0xff) as f32 / 255.0;
    let g = ((color >> 8) & 0xff) as f32 / 255.0;
    let b = (color & 0xff) as f32 / 255.0;
    0.2126 * linear_srgb(r) + 0.7152 * linear_srgb(g) + 0.0722 * linear_srgb(b)
}

fn linear_srgb(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn shape_text_line(
    text: &str,
    font_size: Pixels,
    weight: FontWeight,
    color: gpui::Hsla,
    window: &mut Window,
    profile: &mut PaintProfile,
) -> gpui::ShapedLine {
    let mut font = window.text_style().font();
    font.weight = weight;
    let run = TextRun {
        len: text.len(),
        font,
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let started = profile.enabled.then(Instant::now);
    let line = window
        .text_system()
        .shape_line(SharedString::from(text.to_string()), font_size, &[run], None);
    if let Some(started) = started {
        profile.shape_ms += elapsed_ms(started);
        profile.shape_count += 1;
    }
    line
}

fn elapsed_ms(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1000.0
}

fn metric_value(health: &BTreeMap<String, i64>, key: &str) -> String {
    health
        .get(key)
        .copied()
        .map(format_i64)
        .unwrap_or_else(|| "0".to_string())
}

fn format_i64(value: i64) -> String {
    let chars = value.abs().to_string().chars().rev().collect::<Vec<_>>();
    let mut out = String::new();
    for (ix, ch) in chars.iter().enumerate() {
        if ix > 0 && ix % 3 == 0 {
            out.push(',');
        }
        out.push(*ch);
    }
    let formatted = out.chars().rev().collect::<String>();
    if value < 0 { format!("-{formatted}") } else { formatted }
}
