use std::{collections::HashMap, path::Path, time::Instant};

use anyhow::{Result, anyhow, bail};
use gpui::{Bounds, point, px, size};
use klocc_artifact::Artifact;

use crate::{
    layout::{RectNode, ViewKey, build_layout, hit_test},
    modes::{ColorMode, CompletenessMode, FilterMode, HierarchyMode, MetricMode, cycle},
};

pub(crate) fn exercise_artifact(path: &Path) -> Result<()> {
    let artifact = Artifact::load(path)?;
    let bounds = Bounds::new(point(px(0.0), px(0.0)), size(px(1440.0), px(900.0)));
    let mut cache = HashMap::<ViewKey, Vec<RectNode>>::new();
    let mut cold_ms = 0.0;
    let mut hot_ms = 0.0;
    let mut rect_count = 0usize;
    let mut max_rects = 0usize;
    let mut layout_count = 0usize;

    for metric in MetricMode::ALL {
        for filter in FilterMode::ALL {
            for color in ColorMode::ALL {
                for hierarchy in HierarchyMode::ALL {
                    for completeness in CompletenessMode::ALL {
                        let key = ViewKey {
                            root: None,
                            metric,
                            filter,
                            color,
                            hierarchy,
                            completeness,
                            expanded_tiny_group: None,
                            width: 1440,
                            height: 900,
                        };

                        let started = Instant::now();
                        let rects = build_layout(&artifact, &key, bounds);
                        cold_ms += started.elapsed().as_secs_f64() * 1000.0;
                        rect_count += rects.len();
                        max_rects = max_rects.max(rects.len());
                        layout_count += 1;
                        cache.insert(key.clone(), rects);

                        let started = Instant::now();
                        let Some(cached) = cache.get(&key) else {
                            bail!("layout cache miss for {key:?}");
                        };
                        hot_ms += started.elapsed().as_secs_f64() * 1000.0;
                        rect_count += cached.len();
                    }
                }
            }
        }
    }

    let key = ViewKey {
        root: None,
        metric: MetricMode::Code,
        filter: FilterMode::All,
        color: ColorMode::Layer,
        hierarchy: HierarchyMode::SourceKind,
        completeness: CompletenessMode::Counted,
        expanded_tiny_group: None,
        width: 1440,
        height: 900,
    };
    let root_rects = cache
        .get(&key)
        .cloned()
        .unwrap_or_else(|| build_layout(&artifact, &key, bounds));
    let drill_id = root_rects
        .iter()
        .filter_map(|rect| rect.source_id)
        .find(|id| artifact.has_children(*id))
        .ok_or_else(|| anyhow!("no drillable source found in default layout"))?;
    let drill_rect = root_rects
        .iter()
        .find(|rect| rect.source_id == Some(drill_id))
        .ok_or_else(|| anyhow!("drill source {drill_id} missing from root layout"))?;
    let hit_point = point(
        drill_rect.bounds.origin.x + drill_rect.bounds.size.width * 0.5,
        drill_rect.bounds.origin.y + drill_rect.bounds.size.height * 0.5,
    );
    let hit = hit_test(hit_point, &root_rects).ok_or_else(|| anyhow!("hit test missed drillable rect"))?;
    if hit != crate::layout::HitTarget::Source(drill_id) {
        bail!("hit test returned {hit:?}, expected source {drill_id}");
    }

    let drill_key = ViewKey {
        root: Some(drill_id),
        ..key
    };
    let started = Instant::now();
    let drill_rects = build_layout(&artifact, &drill_key, bounds);
    let drill_ms = started.elapsed().as_secs_f64() * 1000.0;
    if drill_rects.is_empty() {
        bail!("drilldown for source {drill_id} produced no visible dependency rects");
    }

    let cycle_metric = MetricMode::ALL
        .iter()
        .copied()
        .fold(MetricMode::Code, |current, _| cycle(current, &MetricMode::ALL));
    if cycle_metric != MetricMode::Code {
        bail!("metric cycle did not wrap to the initial mode");
    }

    eprintln!(
        "klocc-gui: exercise loaded {} sources in {:.2}ms; {} cold layouts avg {:.3}ms max {} rects; cache probes avg {:.6}ms; hit/drill source {} into {} rects in {:.3}ms; total rect visits {}",
        artifact.sources.len(),
        artifact.loaded_ms,
        layout_count,
        cold_ms / layout_count as f64,
        max_rects,
        hot_ms / layout_count as f64,
        drill_id,
        drill_rects.len(),
        drill_ms,
        rect_count,
    );
    Ok(())
}
