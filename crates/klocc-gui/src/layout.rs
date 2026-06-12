use std::collections::{BTreeMap, BTreeSet};

use gpui::{Bounds, Pixels, Point, point, px, size};
use klocc_artifact::{Artifact, SourceNode, TreemapEntry};

use crate::modes::{ColorMode, CompletenessMode, FilterMode, HierarchyMode, MetricMode};
use crate::theme;

const MIN_RECT_AREA: f64 = 1.0;
const MIN_RENDERED_SIDE: f32 = 3.0;
const TINY_MERGE_SIDE: f32 = 6.0;
const SMALL_GROUPS_KEY: &str = "small-source-kinds";

#[derive(Clone, Debug)]
pub(crate) struct RectNode {
    pub(crate) source_id: Option<i64>,
    pub(crate) label: String,
    pub(crate) group_key: String,
    pub(crate) value: f64,
    pub(crate) color: u32,
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) tiny_group_key: Option<String>,
    pub(crate) tiny_source_ids: Vec<i64>,
    pub(crate) tiny_count: usize,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TinyGroupSelection {
    pub(crate) group_key: String,
    pub(crate) source_ids: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HitTarget {
    Source(i64),
    TinyGroup(TinyGroupSelection),
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct LayoutQuality {
    pub(crate) rects: usize,
    pub(crate) labels: usize,
    pub(crate) slivers: usize,
    pub(crate) tiny: usize,
    pub(crate) min_aspect: f32,
    pub(crate) max_aspect: f32,
}

pub(crate) struct RectDisplayText {
    pub(crate) name: String,
    pub(crate) value: String,
}

#[derive(Debug, Default)]
struct GroupDebugSummary {
    bounds: Option<Bounds<Pixels>>,
    sources: usize,
    tiny_aggregates: usize,
    tiny_sources: usize,
    area: f32,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ViewKey {
    pub(crate) root: Option<i64>,
    pub(crate) metric: MetricMode,
    pub(crate) filter: FilterMode,
    pub(crate) color: ColorMode,
    pub(crate) hierarchy: HierarchyMode,
    pub(crate) completeness: CompletenessMode,
    pub(crate) expanded_tiny_group: Option<TinyGroupSelection>,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

pub(crate) fn build_layout(artifact: &Artifact, key: &ViewKey, bounds: Bounds<Pixels>) -> Vec<RectNode> {
    let mut entries = artifact
        .treemap_entries(key.root, key.hierarchy.view_name())
        .into_iter()
        .filter(|entry| include_entry(entry, key))
        .map(|entry| layout_entry(artifact, &entry, key))
        .collect::<Vec<_>>();

    entries.retain(|entry| {
        entry.value > 0.0
            || matches!(
                key.completeness,
                CompletenessMode::All | CompletenessMode::Generated | CompletenessMode::Missing
            )
    });
    entries.sort_by(|a, b| {
        a.group_key
            .cmp(&b.group_key)
            .then_with(|| b.value.total_cmp(&a.value))
            .then_with(|| a.label.cmp(&b.label))
    });

    if let Some(selection) = key.expanded_tiny_group.as_ref() {
        return layout_expanded_tiny_entries(entries, bounds, selection);
    }

    layout_entries_with_tiny_buckets(entries, bounds)
}

fn layout_expanded_tiny_entries(
    entries: Vec<LayoutEntry>,
    bounds: Bounds<Pixels>,
    selection: &TinyGroupSelection,
) -> Vec<RectNode> {
    let tiny_source_ids = selection.source_ids.iter().copied().collect::<BTreeSet<_>>();
    let entries = entries
        .into_iter()
        .filter(|entry| {
            entry
                .source_id
                .is_some_and(|source_id| tiny_source_ids.contains(&source_id))
        })
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Vec::new();
    }
    layout_entries_with_tiny_buckets(entries, bounds)
}

#[cfg(test)]
fn layout_entries(entries: Vec<LayoutEntry>, bounds: Bounds<Pixels>) -> Vec<RectNode> {
    layout_entries_with_tiny_buckets(entries, bounds)
}

fn layout_entries_with_tiny_buckets(entries: Vec<LayoutEntry>, bounds: Bounds<Pixels>) -> Vec<RectNode> {
    layout_entries_grouped(entries, bounds, true)
}

fn layout_entries_grouped(entries: Vec<LayoutEntry>, bounds: Bounds<Pixels>, merge_tiny: bool) -> Vec<RectNode> {
    let total: f64 = entries.iter().map(|entry| entry.value.max(1.0)).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let mut groups = BTreeMap::<String, Vec<LayoutEntry>>::new();
    for entry in entries {
        groups.entry(entry.group_key.clone()).or_default().push(entry);
    }
    if merge_tiny {
        fold_tiny_top_level_groups(&mut groups, bounds);
    }
    let mut group_entries = groups
        .iter()
        .map(|(group_key, entries)| LayoutEntry {
            source_id: None,
            label: human_group(group_key).to_string(),
            value: entries.iter().map(|entry| entry.value.max(1.0)).sum(),
            color: entries.first().map(|entry| entry.color).unwrap_or(theme::SURFACE1),
            group_key: group_key.clone(),
            tiny_count: 0,
            tiny_source_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    group_entries.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.group_key.cmp(&b.group_key)));

    let group_bounds = squarify(&group_entries, bounds);
    let mut rects = Vec::new();
    for (group, group_bounds) in group_entries.iter().zip(group_bounds) {
        let Some(entries) = groups.get(&group.group_key) else {
            continue;
        };
        let inner_bounds = group_inner_bounds(group_bounds);
        let entries = readable_leaf_entries(entries.clone(), inner_bounds);
        let entries = if merge_tiny && group.group_key != SMALL_GROUPS_KEY {
            entries_with_tiny_bucket(entries, inner_bounds)
        } else {
            entries
        };
        for placement in layout_entries_in_region(&entries, inner_bounds) {
            let entry = placement.entry;
            let tiny_group_key = (entry.tiny_count > 0).then(|| entry.group_key.clone());
            rects.push(RectNode {
                source_id: entry.source_id,
                label: human_label(&entry.label),
                group_key: entry.group_key.clone(),
                value: placement.value,
                color: entry.color,
                bounds: placement.bounds,
                tiny_group_key,
                tiny_source_ids: entry.tiny_source_ids.clone(),
                tiny_count: entry.tiny_count,
            });
        }
    }
    rects
}

fn fold_tiny_top_level_groups(groups: &mut BTreeMap<String, Vec<LayoutEntry>>, bounds: Bounds<Pixels>) {
    let mut group_entries = groups
        .iter()
        .map(|(group_key, entries)| LayoutEntry {
            source_id: None,
            label: human_group(group_key).to_string(),
            value: entries.iter().map(|entry| entry.value.max(1.0)).sum::<f64>(),
            color: entries.first().map(|entry| entry.color).unwrap_or(theme::SURFACE1),
            group_key: group_key.clone(),
            tiny_count: 0,
            tiny_source_ids: Vec::new(),
        })
        .collect::<Vec<_>>();
    group_entries.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.group_key.cmp(&b.group_key)));

    let small_group_keys = group_entries
        .iter()
        .zip(squarify(&group_entries, bounds))
        .filter_map(|(group, bounds)| {
            let inner = group_inner_bounds(bounds);
            (group.group_key != SMALL_GROUPS_KEY && (is_tiny_rect(inner) || is_unreadable_rect(inner)))
                .then(|| group.group_key.clone())
        })
        .collect::<Vec<_>>();
    if small_group_keys.is_empty() {
        return;
    }

    let mut small_entries = Vec::new();
    for group_key in small_group_keys {
        let Some(entries) = groups.remove(&group_key) else {
            continue;
        };
        let Some(first) = entries.first() else {
            continue;
        };
        small_entries.push(tiny_bucket_entry(first, &entries));
    }
    if !small_entries.is_empty() {
        let color = small_entries
            .first()
            .map(|entry| entry.color)
            .unwrap_or(theme::SURFACE1);
        groups.insert(
            SMALL_GROUPS_KEY.to_string(),
            vec![tiny_bucket_entry_for_group(
                SMALL_GROUPS_KEY,
                "tiny small source kinds",
                color,
                &small_entries,
            )],
        );
    }
}

fn entries_with_tiny_bucket(mut entries: Vec<LayoutEntry>, bounds: Bounds<Pixels>) -> Vec<LayoutEntry> {
    let Some(first) = entries.first().cloned() else {
        return entries;
    };
    let mut tiny_entries = Vec::<LayoutEntry>::new();
    loop {
        let mut layout_entries = entries.clone();
        if !tiny_entries.is_empty() {
            layout_entries.push(tiny_bucket_entry(&first, &tiny_entries));
        }
        let placements = layout_entries_in_region(&layout_entries, bounds);
        let tiny_source_ids = placements
            .iter()
            .filter(|placement| {
                placement.entry.source_id.is_some()
                    && (is_tiny_rect(placement.bounds) || is_unreadable_rect(placement.bounds))
            })
            .filter_map(|placement| placement.entry.source_id)
            .collect::<std::collections::BTreeSet<_>>();
        if tiny_source_ids.is_empty() {
            return layout_entries;
        }
        let mut next_entries = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry
                .source_id
                .is_some_and(|source_id| tiny_source_ids.contains(&source_id))
            {
                tiny_entries.push(entry);
            } else {
                next_entries.push(entry);
            }
        }
        entries = next_entries;
    }
}

fn tiny_bucket_entry(first: &LayoutEntry, tiny_entries: &[LayoutEntry]) -> LayoutEntry {
    tiny_bucket_entry_for_group(
        &first.group_key,
        &format!("tiny {}", human_group(&first.group_key)),
        first.color,
        tiny_entries,
    )
}

fn tiny_bucket_entry_for_group(group_key: &str, label: &str, color: u32, tiny_entries: &[LayoutEntry]) -> LayoutEntry {
    let tiny_count = tiny_entries
        .iter()
        .map(|entry| if entry.source_id.is_some() { 1 } else { entry.tiny_count })
        .sum();
    LayoutEntry {
        source_id: None,
        label: format!("{label} ({tiny_count})"),
        value: tiny_entries.iter().map(|entry| entry.value.max(1.0)).sum(),
        color: muted_group_color(color),
        group_key: group_key.to_string(),
        tiny_count,
        tiny_source_ids: tiny_entries
            .iter()
            .flat_map(|entry| entry.source_id.into_iter().chain(entry.tiny_source_ids.iter().copied()))
            .collect(),
    }
}

fn is_tiny_rect(bounds: Bounds<Pixels>) -> bool {
    bounds.size.width.as_f32() < TINY_MERGE_SIDE || bounds.size.height.as_f32() < TINY_MERGE_SIDE
}

fn union_bounds(left: Bounds<Pixels>, right: Bounds<Pixels>) -> Bounds<Pixels> {
    let x = left.left().as_f32().min(right.left().as_f32());
    let y = left.top().as_f32().min(right.top().as_f32());
    let right_edge = left.right().as_f32().max(right.right().as_f32());
    let bottom_edge = left.bottom().as_f32().max(right.bottom().as_f32());
    float_bounds(x, y, right_edge - x, bottom_edge - y)
}

#[cfg(test)]
fn rect_overlap_area(left: Bounds<Pixels>, right: Bounds<Pixels>) -> f32 {
    let width =
        (left.right().as_f32().min(right.right().as_f32()) - left.left().as_f32().max(right.left().as_f32())).max(0.0);
    let height =
        (left.bottom().as_f32().min(right.bottom().as_f32()) - left.top().as_f32().max(right.top().as_f32())).max(0.0);
    width * height
}

fn muted_group_color(color: u32) -> u32 {
    mix_rgb(color, theme::SURFACE2, 0.62)
}

fn mix_rgb(color: u32, target: u32, target_weight: f32) -> u32 {
    let source_weight = 1.0 - target_weight;
    let r =
        (((color >> 16) & 0xff) as f32 * source_weight + ((target >> 16) & 0xff) as f32 * target_weight).round() as u32;
    let g =
        (((color >> 8) & 0xff) as f32 * source_weight + ((target >> 8) & 0xff) as f32 * target_weight).round() as u32;
    let b = ((color & 0xff) as f32 * source_weight + (target & 0xff) as f32 * target_weight).round() as u32;
    (r << 16) | (g << 8) | b
}

#[derive(Clone, Debug)]
struct LayoutEntry {
    source_id: Option<i64>,
    label: String,
    value: f64,
    color: u32,
    group_key: String,
    tiny_count: usize,
    tiny_source_ids: Vec<i64>,
}

fn layout_entry(artifact: &Artifact, entry: &TreemapEntry, key: &ViewKey) -> LayoutEntry {
    let value = metric_for(entry, key.metric).max(if entry.own_code_loc == 0 { 0.0 } else { MIN_RECT_AREA });
    LayoutEntry {
        source_id: entry.source_id,
        label: resolved_entry_label(artifact, entry),
        value,
        color: color_for(entry, key.color),
        group_key: entry.group_key.clone(),
        tiny_count: 0,
        tiny_source_ids: Vec::new(),
    }
}

fn resolved_entry_label(artifact: &Artifact, entry: &TreemapEntry) -> String {
    let label = human_label(&entry.label);
    if !is_placeholder_source_name(&display_name_without_edge(&label)) {
        return label;
    }
    entry
        .source_id
        .and_then(|source_id| context_name_for_source(artifact, source_id))
        .unwrap_or(label)
}

fn context_name_for_source(artifact: &Artifact, source_id: i64) -> Option<String> {
    let mut candidates = artifact
        .outgoing
        .iter()
        .filter_map(|(parent_id, edges)| {
            edges
                .iter()
                .any(|edge| edge.to == source_id)
                .then(|| artifact.source(*parent_id))
                .flatten()
        })
        .map(|source| {
            (
                source_context_rank(source),
                source.own_code_loc,
                human_label(&source.display_name()),
            )
        })
        .filter(|(_, _, label)| !is_placeholder_source_name(&display_name_without_edge(label)))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)));
    candidates
        .into_iter()
        .map(|(_, _, label)| display_name_without_edge(&label))
        .next()
}

fn source_context_rank(source: &SourceNode) -> u8 {
    match source.source_kind.as_str() {
        "derivation-env-src" | "fixed-output-source" => 0,
        "source-like-derivation-output" => 1,
        _ => 2,
    }
}

fn include_entry(entry: &TreemapEntry, key: &ViewKey) -> bool {
    let layer_ok = match key.filter {
        FilterMode::All => true,
        FilterMode::Runtime => entry.runtime_linked,
        FilterMode::Build => entry.build_time_only,
        FilterMode::Unique => entry.unique_transitive_code_loc > 0,
        FilterMode::Shared => entry.shared_transitive_code_loc > 0,
    };
    let complete_ok = match key.completeness {
        CompletenessMode::Counted => entry.own_code_loc > 0,
        CompletenessMode::All => true,
        CompletenessMode::Generated => entry.generated,
        CompletenessMode::Missing => entry.missing,
    };
    layer_ok && complete_ok
}

pub(crate) fn has_visible_children(artifact: &Artifact, source_id: i64, key: &ViewKey) -> bool {
    artifact
        .treemap_entries(Some(source_id), key.hierarchy.view_name())
        .into_iter()
        .any(|entry| {
            include_entry(&entry, key)
                && (metric_for(&entry, key.metric) > 0.0
                    || matches!(
                        key.completeness,
                        CompletenessMode::All | CompletenessMode::Generated | CompletenessMode::Missing
                    ))
        })
}

pub(crate) fn source_count_for_view(artifact: &Artifact, key: &ViewKey) -> usize {
    artifact
        .treemap_entries(key.root, key.hierarchy.view_name())
        .into_iter()
        .filter(|entry| include_entry(entry, key))
        .filter(|entry| {
            metric_for(entry, key.metric) > 0.0
                || matches!(
                    key.completeness,
                    CompletenessMode::All | CompletenessMode::Generated | CompletenessMode::Missing
                )
        })
        .count()
}

fn metric_for(entry: &TreemapEntry, metric: MetricMode) -> f64 {
    (match metric {
        MetricMode::Code => entry.own_code_loc,
        MetricMode::Total => entry.total_code_loc,
        MetricMode::Unique => entry.unique_transitive_code_loc.max(entry.own_code_loc),
        MetricMode::Shared => entry.shared_transitive_code_loc,
    }) as f64
}

fn color_for(entry: &TreemapEntry, color: ColorMode) -> u32 {
    match color {
        ColorMode::Layer if entry.runtime_linked => theme::TEAL,
        ColorMode::Layer => theme::MAUVE,
        ColorMode::Health if entry.generated => theme::YELLOW,
        ColorMode::Health if entry.missing => theme::RED,
        ColorMode::Health => theme::GREEN,
        ColorMode::SourceKind => source_kind_color(&entry.source_kind),
        ColorMode::Ecosystem => ecosystem_color(&entry.ecosystem),
    }
}

fn source_kind_color(source_kind: &str) -> u32 {
    match source_kind {
        "fixed-output-source" => theme::BLUE,
        "source-like-derivation-output" => theme::TEAL,
        "cargo-vendored-crate" => theme::MAUVE,
        "generated-derivation-output" => theme::YELLOW,
        "derivation-env-src" | "derivation-env-srcs" | "derivation-arg-source" => theme::PEACH,
        "patch-source" => theme::RED,
        "nix-input-src" => theme::SKY,
        "cargo-lock-crate" | "cargo-vendor-dir" => theme::PINK,
        "unknown-source" | "unknown-derivation-source" => theme::MAROON,
        _ => hash_color(source_kind),
    }
}

fn ecosystem_color(ecosystem: &str) -> u32 {
    match ecosystem {
        "nix" => theme::BLUE,
        "cargo" => theme::MAUVE,
        "cargo-workspace" => theme::GREEN,
        _ => hash_color(ecosystem),
    }
}

fn hash_color(text: &str) -> u32 {
    let hash = text
        .bytes()
        .fold(0usize, |acc, byte| acc.wrapping_mul(31).wrapping_add(byte as usize));
    theme::CATEGORY_PALETTE[hash % theme::CATEGORY_PALETTE.len()]
}

fn readable_leaf_entries(mut entries: Vec<LayoutEntry>, _bounds: Bounds<Pixels>) -> Vec<LayoutEntry> {
    entries.sort_by(|a, b| b.value.total_cmp(&a.value).then_with(|| a.label.cmp(&b.label)));
    entries
}

struct LocalPlacement<'a> {
    entry: &'a LayoutEntry,
    value: f64,
    bounds: Bounds<Pixels>,
}

fn layout_entries_in_region<'a>(entries: &'a [LayoutEntry], bounds: Bounds<Pixels>) -> Vec<LocalPlacement<'a>> {
    let mut entry_bounds = squarify(entries, bounds);
    if entry_bounds.len() != entries.len() {
        entry_bounds = vec![Bounds::new(bounds.origin, size(px(0.0), px(0.0))); entries.len()];
    }
    entry_bounds
        .into_iter()
        .enumerate()
        .map(|(index, bounds)| LocalPlacement {
            entry: &entries[index],
            value: entries[index].value,
            bounds: leaf_bounds_for(bounds),
        })
        .collect()
}

fn squarify(entries: &[LayoutEntry], bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    if entries.is_empty() || bounds.size.width.as_f32() <= 0.0 || bounds.size.height.as_f32() <= 0.0 {
        return Vec::new();
    }
    let total_value: f64 = entries.iter().map(|entry| entry.value.max(1.0)).sum();
    if total_value <= 0.0 {
        return Vec::new();
    }
    let area = bounds.size.width.as_f32() as f64 * bounds.size.height.as_f32() as f64;
    let mut items = entries
        .iter()
        .enumerate()
        .map(|(index, entry)| TileItem {
            index,
            area: (entry.value.max(1.0) / total_value * area) as f32,
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.area.total_cmp(&a.area));

    let mut output = vec![Bounds::new(bounds.origin, size(px(0.0), px(0.0))); entries.len()];
    let mut free = FloatRect::from_bounds(bounds);
    let mut row = Vec::<TileItem>::new();
    let mut row_sum = 0.0;
    let mut row_min = f32::INFINITY;
    let mut row_max = 0.0;
    for item in items {
        if row.is_empty() {
            push_row_item(&mut row, item, &mut row_sum, &mut row_min, &mut row_max);
            continue;
        }
        let side = free.width.min(free.height).max(1.0);
        if worst_aspect(row_sum, row_min, row_max, side) >= worst_aspect_with(row_sum, row_min, row_max, item, side) {
            push_row_item(&mut row, item, &mut row_sum, &mut row_min, &mut row_max);
        } else {
            layout_row(&row, &mut free, &mut output);
            row.clear();
            row_sum = 0.0;
            row_min = f32::INFINITY;
            row_max = 0.0;
            push_row_item(&mut row, item, &mut row_sum, &mut row_min, &mut row_max);
        }
    }
    if !row.is_empty() {
        layout_row(&row, &mut free, &mut output);
    }
    output
}

#[derive(Clone, Copy)]
struct TileItem {
    index: usize,
    area: f32,
}

fn push_row_item(row: &mut Vec<TileItem>, item: TileItem, sum: &mut f32, min: &mut f32, max: &mut f32) {
    row.push(item);
    *sum += item.area;
    *min = (*min).min(item.area);
    *max = (*max).max(item.area);
}

#[derive(Clone, Copy)]
struct FloatRect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

impl FloatRect {
    fn from_bounds(bounds: Bounds<Pixels>) -> Self {
        Self {
            x: bounds.origin.x.as_f32(),
            y: bounds.origin.y.as_f32(),
            width: bounds.size.width.as_f32(),
            height: bounds.size.height.as_f32(),
        }
    }
}

fn worst_aspect(sum: f32, min: f32, max: f32, side: f32) -> f32 {
    let min = min.max(1.0);
    let max = max.max(1.0);
    let side_squared = side * side;
    ((side_squared * max) / (sum * sum)).max((sum * sum) / (side_squared * min))
}

fn worst_aspect_with(sum: f32, min: f32, max: f32, item: TileItem, side: f32) -> f32 {
    worst_aspect(sum + item.area, min.min(item.area), max.max(item.area), side)
}

fn layout_row(row: &[TileItem], free: &mut FloatRect, output: &mut [Bounds<Pixels>]) {
    let row_area: f32 = row.iter().map(|item| item.area).sum();
    if row_area <= 0.0 || free.width <= 0.0 || free.height <= 0.0 {
        return;
    }
    if free.width >= free.height {
        let row_width = (row_area / free.height).min(free.width);
        let mut y = free.y;
        for item in row {
            let height = if row_width > 0.0 { item.area / row_width } else { 0.0 };
            output[item.index] = float_bounds(free.x, y, row_width, height);
            y += height;
        }
        free.x += row_width;
        free.width -= row_width;
    } else {
        let row_height = (row_area / free.width).min(free.height);
        let mut x = free.x;
        for item in row {
            let width = if row_height > 0.0 { item.area / row_height } else { 0.0 };
            output[item.index] = float_bounds(x, free.y, width, row_height);
            x += width;
        }
        free.y += row_height;
        free.height -= row_height;
    }
}

fn float_bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
    Bounds::new(point(px(x), px(y)), size(px(width.max(0.0)), px(height.max(0.0))))
}

fn group_inner_bounds(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    adaptive_inset(bounds, 2.5)
}

fn leaf_bounds_for(bounds: Bounds<Pixels>) -> Bounds<Pixels> {
    adaptive_inset(bounds, 1.5)
}

fn is_unreadable_rect(bounds: Bounds<Pixels>) -> bool {
    let width = bounds.size.width.as_f32();
    let height = bounds.size.height.as_f32();
    if width < 1.0 || height < 1.0 {
        return true;
    }
    let area = width * height;
    let aspect = (width / height).max(height / width);
    (area >= 64.0 && aspect > 20.0) || (area >= 7_250.0 && aspect > 8.0)
}

fn adaptive_inset(bounds: Bounds<Pixels>, desired_gap: f32) -> Bounds<Pixels> {
    let min_side = bounds.size.width.as_f32().min(bounds.size.height.as_f32());
    let gap = px(if min_side < 8.0 {
        0.0
    } else {
        desired_gap.min(min_side * 0.08)
    });
    Bounds::new(
        point(bounds.origin.x + gap, bounds.origin.y + gap),
        size(
            (bounds.size.width - gap * 2.0).max(px(0.0)),
            (bounds.size.height - gap * 2.0).max(px(0.0)),
        ),
    )
}

pub(crate) fn layout_quality(rects: &[RectNode], label_count: usize) -> LayoutQuality {
    let mut quality = LayoutQuality {
        rects: rects.len(),
        labels: label_count,
        min_aspect: f32::INFINITY,
        ..Default::default()
    };
    for rect in rects {
        if rect.tiny_group_key.is_some() {
            quality.min_aspect = quality.min_aspect.min(1.0);
            quality.max_aspect = quality.max_aspect.max(1.0);
            continue;
        }
        let width = rect.bounds.size.width.as_f32();
        let height = rect.bounds.size.height.as_f32();
        if width < MIN_RENDERED_SIDE || height < MIN_RENDERED_SIDE {
            quality.tiny += 1;
            continue;
        }
        let aspect = (width / height).max(height / width);
        quality.min_aspect = quality.min_aspect.min(aspect);
        quality.max_aspect = quality.max_aspect.max(aspect);
        if is_unreadable_rect(rect.bounds) {
            quality.slivers += 1;
        }
    }
    if !quality.min_aspect.is_finite() {
        quality.min_aspect = 0.0;
    }
    quality
}

pub(crate) fn layout_debug_summary(rects: &[RectNode]) -> String {
    let mut groups = BTreeMap::<&str, GroupDebugSummary>::new();
    let mut sources = 0usize;
    let mut tiny_aggregates = 0usize;
    let mut accounted = 0usize;

    for rect in rects {
        let group = groups.entry(&rect.group_key).or_default();
        group.bounds = Some(match group.bounds {
            Some(bounds) => union_bounds(bounds, rect.bounds),
            None => rect.bounds,
        });
        group.area += rect_area(rect.bounds);
        if rect.source_id.is_some() {
            sources += 1;
            accounted += 1;
            group.sources += 1;
        } else {
            tiny_aggregates += 1;
            accounted += rect.tiny_count;
            group.tiny_aggregates += 1;
            group.tiny_sources += rect.tiny_count;
        }
    }

    let mut groups = groups.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(_, left), (_, right)| right.area.total_cmp(&left.area));
    let groups = groups
        .into_iter()
        .take(8)
        .map(|(group_key, group)| {
            let bounds = group
                .bounds
                .unwrap_or_else(|| Bounds::new(point(px(0.0), px(0.0)), size(px(0.0), px(0.0))));
            format!(
                "{}: src={} tiny_aggs={} tiny_src={} bounds={:.0}x{:.0}@{:.0},{:.0}",
                human_group(group_key),
                group.sources,
                group.tiny_aggregates,
                group.tiny_sources,
                bounds.size.width.as_f32(),
                bounds.size.height.as_f32(),
                bounds.left().as_f32(),
                bounds.top().as_f32()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");

    format!(
        "rects={} sources={} tiny_aggs={} accounted={} groups=[{}]",
        rects.len(),
        sources,
        tiny_aggregates,
        accounted,
        groups
    )
}

pub(crate) fn layout_size_histogram(rects: &[RectNode]) -> String {
    let min_side = bucket_counts(
        rects,
        |rect| rect.bounds.size.width.as_f32().min(rect.bounds.size.height.as_f32()),
        &[
            ("0-1", 0.0, 1.0),
            ("1-2", 1.0, 2.0),
            ("2-3", 2.0, 3.0),
            ("3-6", 3.0, 6.0),
            ("6-12", 6.0, 12.0),
            ("12-24", 12.0, 24.0),
            ("24-48", 24.0, 48.0),
            ("48-96", 48.0, 96.0),
            ("96+", 96.0, f32::INFINITY),
        ],
    );
    let max_side = bucket_counts(
        rects,
        |rect| rect.bounds.size.width.as_f32().max(rect.bounds.size.height.as_f32()),
        &[
            ("0-6", 0.0, 6.0),
            ("6-12", 6.0, 12.0),
            ("12-24", 12.0, 24.0),
            ("24-48", 24.0, 48.0),
            ("48-96", 48.0, 96.0),
            ("96-192", 96.0, 192.0),
            ("192-384", 192.0, 384.0),
            ("384+", 384.0, f32::INFINITY),
        ],
    );
    let area = bucket_counts(
        rects,
        |rect| rect_area(rect.bounds),
        &[
            ("0-4", 0.0, 4.0),
            ("4-16", 4.0, 16.0),
            ("16-64", 16.0, 64.0),
            ("64-256", 64.0, 256.0),
            ("256-1k", 256.0, 1_024.0),
            ("1k-4k", 1_024.0, 4_096.0),
            ("4k-16k", 4_096.0, 16_384.0),
            ("16k-64k", 16_384.0, 65_536.0),
            ("64k+", 65_536.0, f32::INFINITY),
        ],
    );
    format!(
        "\nmin side px\n{}\nmax side px\n{}\narea px^2\n{}",
        render_buckets(&min_side),
        render_buckets(&max_side),
        render_buckets(&area)
    )
}

pub(crate) fn layout_rect_manifest(rects: &[RectNode]) -> String {
    let mut rects = rects.iter().collect::<Vec<_>>();
    rects.sort_by(|left, right| {
        left.bounds
            .top()
            .as_f32()
            .total_cmp(&right.bounds.top().as_f32())
            .then_with(|| left.bounds.left().as_f32().total_cmp(&right.bounds.left().as_f32()))
            .then_with(|| left.group_key.cmp(&right.group_key))
            .then_with(|| left.label.cmp(&right.label))
    });

    let mut lines = Vec::with_capacity(rects.len() + 2);
    lines.push(format!("rect manifest begin count={}", rects.len()));
    for (index, rect) in rects.into_iter().enumerate() {
        let kind = if rect.source_id.is_some() { "source" } else { "tiny" };
        let id = rect
            .source_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| rect.tiny_group_key.as_deref().unwrap_or("tiny").to_string());
        let display_text = rect_display_text(rect)
            .map(|text| format!("{} / {}", text.name, text.value))
            .unwrap_or_else(|| "none".to_string());
        lines.push(format!(
            "  {index:04} {kind:<6} id={id:<24} group={:<31} color=#{:06x} x={:>7.1} y={:>7.1} w={:>7.1} h={:>7.1} min={:>6.1} max={:>6.1} area={:>9.1} value={:>12.1} tiny_count={:>4} display_text={} label={}",
            rect.group_key,
            rect.color,
            rect.bounds.left().as_f32(),
            rect.bounds.top().as_f32(),
            rect.bounds.size.width.as_f32(),
            rect.bounds.size.height.as_f32(),
            rect.bounds.size.width.as_f32().min(rect.bounds.size.height.as_f32()),
            rect.bounds.size.width.as_f32().max(rect.bounds.size.height.as_f32()),
            rect_area(rect.bounds),
            rect.value,
            rect.tiny_count,
            display_text.replace('\n', " "),
            rect.label.replace('\n', " ")
        ));
    }
    lines.push("rect manifest end".to_string());
    lines.join("\n")
}

pub(crate) fn rect_display_text(rect: &RectNode) -> Option<RectDisplayText> {
    if rect.tiny_group_key.is_some() {
        return None;
    }
    if rect.bounds.size.width.as_f32() < 34.0 || rect.bounds.size.height.as_f32() < 25.0 {
        return None;
    }
    let value = compact_value(rect.value as i64);
    let name = display_name_without_edge(&rect.label);
    Some(RectDisplayText { name, value })
}

fn compact_value(value: i64) -> String {
    let abs = value.abs();
    let formatted = if abs >= 1_000_000_000 {
        format!("{:.1}B", value as f64 / 1_000_000_000.0)
    } else if abs >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if abs >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    };
    formatted.replace(".0", "")
}

pub(crate) fn layout_ascii_map(rects: &[RectNode], columns: usize, rows: usize) -> String {
    if rects.is_empty() || columns == 0 || rows == 0 {
        return "ascii map unavailable".to_string();
    }
    let bounds = rects.iter().fold(None, |bounds, rect| {
        Some(match bounds {
            Some(bounds) => union_bounds(bounds, rect.bounds),
            None => rect.bounds,
        })
    });
    let Some(bounds) = bounds else {
        return "ascii map unavailable".to_string();
    };
    let width = bounds.size.width.as_f32().max(1.0);
    let height = bounds.size.height.as_f32().max(1.0);
    let group_symbols = group_symbols(rects);
    let mut lines = Vec::with_capacity(rows + group_symbols.len() + 4);
    lines.push(format!(
        "ascii map {}x{} over {:.0}x{:.0}@{:.0},{:.0} (* = tiny aggregate)",
        columns,
        rows,
        width,
        height,
        bounds.left().as_f32(),
        bounds.top().as_f32()
    ));
    for row in 0..rows {
        let y = bounds.top().as_f32() + (row as f32 + 0.5) / rows as f32 * height;
        let mut line = String::with_capacity(columns);
        for column in 0..columns {
            let x = bounds.left().as_f32() + (column as f32 + 0.5) / columns as f32 * width;
            let ch = rects
                .iter()
                .rev()
                .find(|rect| point_in_bounds(x, y, rect.bounds))
                .map(|rect| {
                    if rect.tiny_group_key.is_some() {
                        '*'
                    } else {
                        *group_symbols.get(rect.group_key.as_str()).unwrap_or(&'?')
                    }
                })
                .unwrap_or('.');
            line.push(ch);
        }
        lines.push(line);
    }
    lines.push("legend:".to_string());
    let mut legend = group_symbols.into_iter().collect::<Vec<_>>();
    legend.sort_by_key(|(_, symbol)| *symbol);
    for (group_key, symbol) in legend {
        lines.push(format!("  {symbol} = {} ({group_key})", human_group(group_key)));
    }
    lines.join("\n")
}

fn group_symbols(rects: &[RectNode]) -> BTreeMap<&str, char> {
    const SYMBOLS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789abcdefghijklmnopqrstuvwxyz";
    let mut symbols = BTreeMap::new();
    for rect in rects {
        let next_index = symbols.len();
        symbols
            .entry(rect.group_key.as_str())
            .or_insert_with(|| SYMBOLS.get(next_index).copied().map(char::from).unwrap_or('?'));
    }
    symbols
}

fn point_in_bounds(x: f32, y: f32, bounds: Bounds<Pixels>) -> bool {
    x >= bounds.left().as_f32()
        && x <= bounds.right().as_f32()
        && y >= bounds.top().as_f32()
        && y <= bounds.bottom().as_f32()
}

fn bucket_counts(
    rects: &[RectNode],
    value: impl Fn(&RectNode) -> f32,
    ranges: &[(&'static str, f32, f32)],
) -> Vec<(&'static str, usize, usize)> {
    ranges
        .iter()
        .map(|(label, min, max)| {
            let mut sources = 0usize;
            let mut tiny_aggregates = 0usize;
            for rect in rects {
                let value = value(rect);
                if value >= *min && value < *max {
                    if rect.source_id.is_some() {
                        sources += 1;
                    } else {
                        tiny_aggregates += 1;
                    }
                }
            }
            (*label, sources, tiny_aggregates)
        })
        .collect()
}

fn render_buckets(buckets: &[(&'static str, usize, usize)]) -> String {
    let max_count = buckets
        .iter()
        .map(|(_, sources, tiny_aggregates)| sources + tiny_aggregates)
        .max()
        .unwrap_or(1)
        .max(1);
    buckets
        .iter()
        .map(|(label, sources, tiny_aggregates)| {
            let total = sources + tiny_aggregates;
            let bar_len = ((total as f32 / max_count as f32) * 42.0).round() as usize;
            format!(
                "  {label:>8} | {total:>4} src={sources:>4} tiny_agg={tiny_aggregates:>2} | {}",
                "#".repeat(bar_len)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rect_area(bounds: Bounds<Pixels>) -> f32 {
    bounds.size.width.as_f32() * bounds.size.height.as_f32()
}

pub(crate) fn human_label(label: &str) -> String {
    let Some((base, edge)) = label.split_once(" via ") else {
        return human_name(base_name(label));
    };
    format!("{} via {edge}", human_name(base_name(base)))
}

fn display_name_without_edge(label: &str) -> String {
    let base = label.split_once(" via ").map(|(base, _)| base).unwrap_or(label);
    human_name(base_name(base))
}

fn human_name(label: &str) -> String {
    let mut label = strip_nix_hash_prefix(label).unwrap_or(label).to_string();
    if let Some(stripped) = label.strip_prefix("cargo-package-") {
        label = stripped.to_string();
    }
    loop {
        let mut stripped_suffix = false;
        for suffix in [".tar.gz", ".tar.xz", ".tar.bz2", ".tgz", ".zip"] {
            if let Some(stripped) = label.strip_suffix(suffix) {
                label = stripped.to_string();
                stripped_suffix = true;
                break;
            }
        }
        if !stripped_suffix {
            break;
        }
    }
    for suffix in ["-src", "-source"] {
        if let Some(stripped) = label.strip_suffix(suffix)
            && !is_placeholder_source_name(stripped)
        {
            label = stripped.to_string();
            break;
        }
    }
    for marker in ["-src-", "-source-"] {
        if let Some((prefix, suffix)) = label.split_once(marker)
            && suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit())
            && !is_placeholder_source_name(prefix)
        {
            label = format!("{prefix}-{suffix}");
            break;
        }
    }
    label
}

fn base_name(label: &str) -> &str {
    label.rsplit('/').next().unwrap_or(label)
}

fn is_placeholder_source_name(label: &str) -> bool {
    matches!(label, "source" | "src")
}

pub(crate) fn human_group(group_key: &str) -> &str {
    match group_key {
        "fixed-output-source" => "fixed output",
        "source-like-derivation-output" => "source output",
        "cargo-vendored-crate" => "cargo vendor",
        "generated-derivation-output" => "generated",
        "derivation-env-src" => "drv source",
        "derivation-env-srcs" => "drv sources",
        "derivation-arg-source" => "drv arg source",
        "patch-source" => "patch",
        "nix-input-src" => "nix input",
        "cargo-lock-crate" => "cargo lock",
        "cargo-vendor-dir" => "cargo vendor dir",
        "tiny-sources" => "tiny sources",
        "small-source-kinds" => "small source kinds",
        other => other,
    }
}

fn strip_nix_hash_prefix(label: &str) -> Option<&str> {
    let (prefix, rest) = label.split_once('-')?;
    (prefix.len() >= 20 && prefix.chars().all(|ch| ch.is_ascii_alphanumeric())).then_some(rest)
}

pub(crate) fn hit_test(point: Point<Pixels>, rects: &[RectNode]) -> Option<HitTarget> {
    rects.iter().rev().find_map(|rect| {
        let hit = point.x >= rect.bounds.left()
            && point.x <= rect.bounds.right()
            && point.y >= rect.bounds.top()
            && point.y <= rect.bounds.bottom();
        if !hit {
            return None;
        }
        if let Some(source_id) = rect.source_id {
            return Some(HitTarget::Source(source_id));
        }
        rect.tiny_group_key.as_ref().map(|group_key| {
            HitTarget::TinyGroup(TinyGroupSelection {
                group_key: group_key.clone(),
                source_ids: rect.tiny_source_ids.clone(),
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;

    const TEST_COLORS: [u32; 8] = [
        theme::BLUE,
        theme::PEACH,
        theme::MAUVE,
        theme::YELLOW,
        theme::TEAL,
        theme::PINK,
        theme::SKY,
        theme::RED,
    ];

    proptest! {
        #[test]
        fn randomized_layout_stays_in_bounds(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_rects_stay_in_bounds(&rects, bounds)?;
        }

        #[test]
        fn randomized_layout_preserves_visible_area_proportions(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_area_matches_value_scale(&rects)?;
        }

        #[test]
        fn randomized_layout_rectangles_do_not_overlap(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_rectangles_do_not_overlap(&rects)?;
        }

        #[test]
        fn randomized_color_blocks_are_rectangularly_filled(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_color_blocks_are_rectangularly_filled(&rects)?;
        }

        #[test]
        fn randomized_layout_keeps_each_color_group_spatially_local(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_color_group_locality(&rects)?;
        }

        #[test]
        fn randomized_layout_renders_one_rect_per_dependency(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let input_count = entries.len();
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_dependency_count_is_conserved(input_count, &rects)?;
        }

        #[test]
        fn randomized_layout_merges_tiny_source_rects(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_no_tiny_source_rects(&rects)?;
        }

        #[test]
        fn randomized_layout_has_at_most_one_tiny_aggregate_per_group(case in layout_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_one_tiny_aggregate_per_group(&rects)?;
        }

        #[test]
        fn drilled_ratio_layout_does_not_emit_source_slivers(case in drilled_ratio_case_strategy()) {
            let (entries, bounds) = case;
            let rects = layout_entries(entries, bounds);
            prop_assert!(!rects.is_empty());
            assert_no_unreadable_source_rects(&rects)?;
            assert_no_tiny_aggregate_strips(&rects)?;
        }

    }

    #[test]
    fn regression_drilled_layout_does_not_emit_source_slivers() {
        let rects = layout_entries(drilled_regression_entries(), drilled_regression_bounds());
        assert_eq!(layout_quality(&rects, 0).slivers, 0, "rects={rects:#?}");
    }

    #[test]
    fn regression_drilled_layout_does_not_emit_tiny_aggregate_strips() {
        let rects = layout_entries(drilled_regression_entries(), drilled_regression_bounds());
        assert!(
            rects
                .iter()
                .filter(|rect| rect.tiny_group_key.is_some())
                .all(|rect| !is_tiny_rect(rect.bounds)),
            "rects={rects:#?}"
        );
    }

    #[test]
    fn locality_assertion_rejects_split_color_islands() {
        let rects = vec![
            test_rect("source-kind-0", theme::BLUE, 0.0, 0.0, 120.0, 120.0),
            test_rect("source-kind-0", theme::BLUE, 1000.0, 700.0, 120.0, 120.0),
        ];
        assert!(assert_color_group_locality(&rects).is_err());
    }

    #[test]
    fn area_scale_assertion_rejects_equal_area_for_wildly_different_values() {
        let rects = vec![
            test_rect_with_value("source-kind-0", theme::BLUE, 0.0, 0.0, 100.0, 100.0, 14_000_000.0),
            test_rect_with_value("source-kind-1", theme::PEACH, 120.0, 0.0, 100.0, 100.0, 14_100.0),
        ];
        assert!(assert_area_matches_value_scale(&rects).is_err());
    }

    #[test]
    fn area_scale_assertion_rejects_tiny_aggregate_inflation() {
        let rects = vec![
            test_rect_with_value("derivation-env-src", theme::PEACH, 0.0, 0.0, 160.0, 120.0, 12_400_000.0),
            test_tiny_rect_with_value("derivation-env-srcs", theme::PEACH, 180.0, 0.0, 150.0, 120.0, 17_500.0),
        ];
        assert!(assert_area_matches_value_scale(&rects).is_err());
    }

    #[test]
    fn overlap_assertion_rejects_overlapping_cells() {
        let rects = vec![
            test_rect("source-kind-0", theme::BLUE, 0.0, 0.0, 100.0, 100.0),
            test_rect("source-kind-0", theme::BLUE, 70.0, 70.0, 30.0, 30.0),
        ];
        assert!(assert_rectangles_do_not_overlap(&rects).is_err());
    }

    fn layout_case_strategy() -> impl Strategy<Value = (Vec<LayoutEntry>, Bounds<Pixels>)> {
        (
            900_u32..=1800,
            600_u32..=1100,
            prop::collection::vec((0_usize..8, 0_u32..7, 1_u32..100), 24..260),
        )
            .prop_map(|(width, height, raw_entries)| {
                let entries = raw_entries
                    .into_iter()
                    .enumerate()
                    .map(|(index, (group_index, magnitude, mantissa))| {
                        let value = mantissa as f64 * 10_f64.powi(magnitude as i32);
                        LayoutEntry {
                            source_id: Some(index as i64),
                            label: format!("source-{group_index}-{index}"),
                            value,
                            color: TEST_COLORS[group_index],
                            group_key: format!("source-kind-{group_index}"),
                            tiny_count: 0,
                            tiny_source_ids: Vec::new(),
                        }
                    })
                    .collect::<Vec<_>>();
                let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(width as f32), px(height as f32)));
                (entries, bounds)
            })
    }

    fn drilled_ratio_case_strategy() -> impl Strategy<Value = (Vec<LayoutEntry>, Bounds<Pixels>)> {
        (
            10_000_000.0..=14_000_000.0,
            220_000.0..=520_000.0,
            220_000.0..=520_000.0,
            30_000.0..=70_000.0,
            900.0..=1_600.0,
            900.0..=1_600.0,
        )
            .prop_map(|(dominant, medium_a, medium_b, small, tiny_a, tiny_b)| {
                let entries = vec![
                    test_entry("derivation-env-src", theme::PEACH, 25, dominant),
                    test_entry("derivation-env-src", theme::PEACH, 553, medium_a),
                    test_entry("derivation-env-src", theme::PEACH, 413, small),
                    test_entry("fixed-output-source", theme::BLUE, 77, medium_b),
                    test_entry("nix-input-src", theme::SKY, 1, tiny_a),
                    test_entry("nix-input-src", theme::SKY, 2, tiny_b),
                ];
                (entries, drilled_regression_bounds())
            })
    }

    fn drilled_regression_entries() -> Vec<LayoutEntry> {
        vec![
            test_entry("derivation-env-src", theme::PEACH, 25, 11_410_032.0),
            test_entry("derivation-env-src", theme::PEACH, 553, 293_508.0),
            test_entry("derivation-env-src", theme::PEACH, 413, 40_610.0),
            test_entry("fixed-output-source", theme::BLUE, 77, 396_775.0),
            test_entry("nix-input-src", theme::SKY, 1, 1_100.0),
            test_entry("nix-input-src", theme::SKY, 2, 1_074.0),
        ]
    }

    fn drilled_regression_bounds() -> Bounds<Pixels> {
        Bounds::new(point(px(0.0), px(0.0)), size(px(1416.0), px(820.0)))
    }

    fn assert_rects_stay_in_bounds(rects: &[RectNode], bounds: Bounds<Pixels>) -> Result<(), TestCaseError> {
        let left = bounds.left().as_f32() - 0.5;
        let right = bounds.right().as_f32() + 0.5;
        let top = bounds.top().as_f32() - 0.5;
        let bottom = bounds.bottom().as_f32() + 0.5;
        for rect in rects {
            let rect_left = rect.bounds.left().as_f32();
            let rect_right = rect.bounds.right().as_f32();
            let rect_top = rect.bounds.top().as_f32();
            let rect_bottom = rect.bounds.bottom().as_f32();
            prop_assert!(rect_left >= left, "rect escaped left bound: {rect:?}");
            prop_assert!(rect_right <= right, "rect escaped right bound: {rect:?}");
            prop_assert!(rect_top >= top, "rect escaped top bound: {rect:?}");
            prop_assert!(rect_bottom <= bottom, "rect escaped bottom bound: {rect:?}");
        }
        Ok(())
    }

    fn assert_area_matches_value_scale(rects: &[RectNode]) -> Result<(), TestCaseError> {
        let measurable = rects
            .iter()
            .filter(|rect| rect.value > 0.0 && area(rect.bounds) >= 9.0)
            .collect::<Vec<_>>();
        for (left_index, left) in measurable.iter().enumerate() {
            for right in measurable.iter().skip(left_index + 1) {
                let value_ratio = (left.value / right.value).max(right.value / left.value);
                if value_ratio < 50.0 {
                    continue;
                }
                let area_ratio =
                    (area(left.bounds) / area(right.bounds)).max(area(right.bounds) / area(left.bounds)) as f64;
                prop_assert!(
                    area_ratio >= value_ratio / 8.0,
                    "rendered area is detached from selected metric value: value_ratio={value_ratio:.2} area_ratio={area_ratio:.2} left={left:?} right={right:?}"
                );
            }
        }
        Ok(())
    }

    fn assert_rectangles_do_not_overlap(rects: &[RectNode]) -> Result<(), TestCaseError> {
        for (left_index, left) in rects.iter().enumerate() {
            for right in rects.iter().skip(left_index + 1) {
                let overlap = rect_overlap_area(left.bounds, right.bounds);
                prop_assert!(
                    overlap <= 0.5,
                    "rectangles overlap instead of occupying separate layout cells: overlap={overlap:.2} left={left:?} right={right:?}"
                );
            }
        }
        Ok(())
    }

    fn assert_one_tiny_aggregate_per_group(rects: &[RectNode]) -> Result<(), TestCaseError> {
        let mut counts = BTreeMap::<&str, usize>::new();
        for rect in rects.iter().filter(|rect| rect.tiny_group_key.is_some()) {
            *counts.entry(&rect.group_key).or_default() += 1;
        }
        for (group_key, count) in counts {
            prop_assert_eq!(
                count,
                1,
                "more than one tiny aggregate for group {}: rects={:?}",
                group_key,
                rects
            );
        }
        Ok(())
    }

    fn assert_color_blocks_are_rectangularly_filled(rects: &[RectNode]) -> Result<(), TestCaseError> {
        let mut groups = BTreeMap::<&str, Vec<&RectNode>>::new();
        for rect in rects {
            groups.entry(&rect.group_key).or_default().push(rect);
        }

        for (group_key, group_rects) in groups {
            if group_rects.len() < 2 {
                continue;
            }
            let union = union_bounds(&group_rects);
            let union_width = union.size.width.as_f32();
            let union_height = union.size.height.as_f32();
            if union_width < 80.0 || union_height < 80.0 {
                continue;
            }

            for y in sample_rows(union) {
                let coverage = row_coverage(&group_rects, y, union);
                prop_assert!(
                    coverage >= 0.55,
                    "color block has a mostly empty row instead of a filled rectangular block: group={group_key} y={y:.2} coverage={coverage:.3} union={union:?} rects={}",
                    group_rects.len()
                );
            }
        }
        Ok(())
    }

    fn assert_dependency_count_is_conserved(input_count: usize, rects: &[RectNode]) -> Result<(), TestCaseError> {
        let accounted = rects
            .iter()
            .map(|rect| if rect.source_id.is_some() { 1 } else { rect.tiny_count })
            .sum::<usize>();
        prop_assert!(
            rects
                .iter()
                .all(|rect| rect.source_id.is_some() || rect.tiny_group_key.is_some()),
            "layout emitted a non-source rect that is not a tiny aggregate: rects={rects:?}"
        );
        prop_assert_eq!(
            accounted,
            input_count,
            "dependency count changed: input={} accounted={} rects={:?}",
            input_count,
            accounted,
            rects
        );
        Ok(())
    }

    fn assert_no_tiny_source_rects(rects: &[RectNode]) -> Result<(), TestCaseError> {
        for rect in rects.iter().filter(|rect| rect.source_id.is_some()) {
            prop_assert!(
                !is_tiny_rect(rect.bounds),
                "tiny source rect should have been merged: {rect:?}"
            );
        }
        Ok(())
    }

    fn assert_no_unreadable_source_rects(rects: &[RectNode]) -> Result<(), TestCaseError> {
        for rect in rects.iter().filter(|rect| rect.source_id.is_some()) {
            prop_assert!(
                !is_unreadable_rect(rect.bounds),
                "source sliver should have been merged: {rect:?}"
            );
        }
        Ok(())
    }

    fn assert_no_tiny_aggregate_strips(rects: &[RectNode]) -> Result<(), TestCaseError> {
        for rect in rects.iter().filter(|rect| rect.tiny_group_key.is_some()) {
            prop_assert!(
                !is_tiny_rect(rect.bounds),
                "tiny aggregate strip should have been merged: {rect:?}"
            );
        }
        Ok(())
    }

    fn sample_rows(bounds: Bounds<Pixels>) -> [f32; 5] {
        let top = bounds.top().as_f32();
        let height = bounds.size.height.as_f32();
        [
            top + height * 0.10,
            top + height * 0.35,
            top + height * 0.50,
            top + height * 0.65,
            top + height * 0.90,
        ]
    }

    fn row_coverage(rects: &[&RectNode], y: f32, union: Bounds<Pixels>) -> f32 {
        let gap_tolerance = 2.0;
        let mut intervals = rects
            .iter()
            .filter_map(|rect| {
                (y >= rect.bounds.top().as_f32() - gap_tolerance && y <= rect.bounds.bottom().as_f32() + gap_tolerance)
                    .then_some((
                        rect.bounds.left().as_f32().max(union.left().as_f32()),
                        rect.bounds.right().as_f32().min(union.right().as_f32()),
                    ))
            })
            .collect::<Vec<_>>();
        intervals.sort_by(|left, right| left.0.total_cmp(&right.0));

        let mut covered = 0.0;
        let mut current: Option<(f32, f32)> = None;
        for (start, end) in intervals {
            current = Some(match current {
                Some((current_start, current_end)) if start <= current_end => (current_start, current_end.max(end)),
                Some((current_start, current_end)) => {
                    covered += current_end - current_start;
                    (start, end)
                }
                None => (start, end),
            });
        }
        if let Some((start, end)) = current {
            covered += end - start;
        }
        covered / union.size.width.as_f32().max(1.0)
    }

    fn assert_color_group_locality(rects: &[RectNode]) -> Result<(), TestCaseError> {
        let mut groups = BTreeMap::<(&str, u32), Vec<&RectNode>>::new();
        for rect in rects {
            if rect.tiny_group_key.is_some() {
                continue;
            }
            groups.entry((&rect.group_key, rect.color)).or_default().push(rect);
        }

        for ((group_key, color), rects) in groups {
            if rects.len() < 2 {
                continue;
            }
            let union = union_bounds(&rects);
            let union_area = area(union).max(1.0);
            let rect_area: f32 = rects.iter().map(|rect| area(rect.bounds)).sum();
            let fill_ratio = rect_area / union_area;
            prop_assert!(
                fill_ratio >= 0.15,
                "same color/group is split across distant islands: group={group_key} color={color:#x} fill_ratio={fill_ratio:.3} rects={}",
                rects.len()
            );
        }
        Ok(())
    }

    fn union_bounds(rects: &[&RectNode]) -> Bounds<Pixels> {
        let left = rects
            .iter()
            .map(|rect| rect.bounds.left().as_f32())
            .fold(f32::INFINITY, f32::min);
        let top = rects
            .iter()
            .map(|rect| rect.bounds.top().as_f32())
            .fold(f32::INFINITY, f32::min);
        let right = rects
            .iter()
            .map(|rect| rect.bounds.right().as_f32())
            .fold(0.0, f32::max);
        let bottom = rects
            .iter()
            .map(|rect| rect.bounds.bottom().as_f32())
            .fold(0.0, f32::max);
        Bounds::new(point(px(left), px(top)), size(px(right - left), px(bottom - top)))
    }

    fn area(bounds: Bounds<Pixels>) -> f32 {
        bounds.size.width.as_f32() * bounds.size.height.as_f32()
    }

    fn test_rect(group_key: &str, color: u32, x: f32, y: f32, width: f32, height: f32) -> RectNode {
        test_rect_with_value(group_key, color, x, y, width, height, width as f64 * height as f64)
    }

    fn test_entry(group_key: &str, color: u32, source_id: i64, value: f64) -> LayoutEntry {
        LayoutEntry {
            source_id: Some(source_id),
            label: format!("source-{source_id}"),
            value,
            color,
            group_key: group_key.to_string(),
            tiny_count: 0,
            tiny_source_ids: Vec::new(),
        }
    }

    fn test_rect_with_value(
        group_key: &str,
        color: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        value: f64,
    ) -> RectNode {
        RectNode {
            source_id: Some(1),
            label: group_key.to_string(),
            group_key: group_key.to_string(),
            value,
            color,
            bounds: Bounds::new(point(px(x), px(y)), size(px(width), px(height))),
            tiny_group_key: None,
            tiny_source_ids: Vec::new(),
            tiny_count: 0,
        }
    }

    fn test_tiny_rect_with_value(
        group_key: &str,
        color: u32,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        value: f64,
    ) -> RectNode {
        RectNode {
            source_id: None,
            label: group_key.to_string(),
            group_key: group_key.to_string(),
            value,
            color: muted_group_color(color),
            bounds: Bounds::new(point(px(x), px(y)), size(px(width), px(height))),
            tiny_group_key: Some(group_key.to_string()),
            tiny_source_ids: vec![1, 2],
            tiny_count: 2,
        }
    }
}
