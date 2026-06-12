def sqlite_metric(sql):
    return int(machine.succeed(q(sqlite3_bin) + " " + q(gui_artifact_path) + " " + q(sql)).strip() or "0")


def assert_artifact_contract():
    artifact = scenario_assertions.get("artifact", {})
    if not artifact:
        return
    source_units_with_loc = sqlite_metric("SELECT value FROM scan_health WHERE metric = 'source_units_with_loc';")
    total_code_loc = sqlite_metric("SELECT COALESCE(SUM(loc_code), 0) FROM source_loc;")
    min_source_units_with_loc = artifact.get("minSourceUnitsWithLoc")
    min_total_code_loc = artifact.get("minTotalCodeLoc")
    if min_source_units_with_loc is not None and source_units_with_loc < min_source_units_with_loc:
        raise Exception("source_units_with_loc " + str(source_units_with_loc) + " below " + str(min_source_units_with_loc))
    if min_total_code_loc is not None and total_code_loc < min_total_code_loc:
        raise Exception("total_code_loc " + str(total_code_loc) + " below " + str(min_total_code_loc))


def assert_gui_telemetry(log_path):
    telemetry = scenario_assertions.get("telemetry", {})
    if not telemetry:
        return
    loaded_sources = int(machine.succeed("awk '/klocc-gui: loaded/ { print $3; exit }' " + q(log_path)).strip() or "0")
    min_loaded_sources = telemetry.get("minLoadedSources")
    if min_loaded_sources is not None and loaded_sources < min_loaded_sources:
        raise Exception("loaded sources " + str(loaded_sources) + " below " + str(min_loaded_sources))

    layout_count = int(machine.succeed("awk '/klocc-gui: layout/ { count++ } END { print count + 0 }' " + q(log_path)).strip())
    min_layout_passes = telemetry.get("minLayoutPasses")
    if min_layout_passes is not None and layout_count < min_layout_passes:
        raise Exception("layout passes " + str(layout_count) + " below " + str(min_layout_passes))

    rect_summary = machine.succeed("awk '/klocc-gui: layout/ { layout_count++; for (i = 1; i <= NF; i++) if ($i == \"rects\") { rects = $(i - 1) + 0; if (rects == 0) zero_layouts++; if (rects > max_rects) max_rects = rects; } } END { printf \"%d %d %d\", layout_count + 0, zero_layouts + 0, max_rects + 0 }' " + q(log_path)).split()
    logged_layouts, zero_layouts, max_rects = [int(value) for value in rect_summary]
    if logged_layouts == 0:
        raise Exception("GUI produced no layout telemetry")
    if telemetry.get("requireNoZeroRectLayouts", False) and zero_layouts != 0:
        raise Exception(str(zero_layouts) + " layout passes produced zero treemap rects")
    min_max_rects = telemetry.get("minMaxRects")
    if min_max_rects is not None and max_rects < min_max_rects:
        raise Exception("max rects " + str(max_rects) + " below " + str(min_max_rects))

    quality_summary = machine.succeed("awk '/klocc-gui: paint quality/ { quality_count++; if ($7 + 0 > max_labels) max_labels = $7 + 0; if ($9 + 0 > max_slivers) max_slivers = $9 + 0; if ($11 + 0 > max_tiny) max_tiny = $11 + 0; } END { printf \"%d %d %d %d\", quality_count + 0, max_labels + 0, max_slivers + 0, max_tiny + 0 }' " + q(log_path)).split()
    quality_count, max_labels, observed_slivers, observed_tiny = [int(value) for value in quality_summary]
    if telemetry.get("requirePaintQuality", False) and quality_count == 0:
        raise Exception("GUI produced no paint quality telemetry")
    min_paint_labels = telemetry.get("minPaintLabels")
    if min_paint_labels is not None and max_labels < min_paint_labels:
        raise Exception("paint labels " + str(max_labels) + " below " + str(min_paint_labels))
    max_slivers = telemetry.get("maxSlivers")
    if max_slivers is not None and observed_slivers > max_slivers:
        raise Exception("slivers " + str(observed_slivers) + " above " + str(max_slivers))
    max_tiny = telemetry.get("maxTiny")
    if max_tiny is not None and observed_tiny > max_tiny:
        raise Exception("tiny rects " + str(observed_tiny) + " above " + str(max_tiny))


def assert_image_stats(stats):
    images = scenario_assertions.get("images", {})
    if images.get("requireSizeMatchesScreen", True):
        machine.succeed("grep -q '" + screen_size + "' " + q(stats))
    assert_nonblank_stats(stats)


def assert_nonblank_stats(stats):
    min_stddev = scenario_assertions.get("images", {}).get("minStddev", 1000)
    machine.succeed("awk '$2 > " + str(min_stddev) + " { ok=1 } END { exit !ok }' " + q(stats))


def log_line_count(log_path):
    return int(machine.succeed("wc -l < " + q(log_path)).strip() or "0")


def extract_rect_manifest(log_path, output_path, start_line, scenario_id, step_id, status):
    awk = r'''
NR <= start { next }
/^klocc-gui: layout / { layout = $0 }
/^rect manifest begin/ { in_block = 1; block = $0 "\n"; block_start = NR; next }
in_block {
  block = block $0 "\n"
  if ($0 == "rect manifest end") {
    last = block
    last_start = block_start
    last_end = NR
    last_layout = layout
    in_block = 0
  }
}
END {
  if (last == "") exit 42
  print "scenario=" scenario
  print "step=" step
  print "status=" status
  print "cursor_start=" start
  print "log_start=" last_start
  print "log_end=" last_end
  if (last_layout != "") print "layout_line=" last_layout
  printf "%s", last
}
'''
    command = (
        "awk -v start="
        + str(start_line)
        + " -v scenario="
        + q(scenario_id)
        + " -v step="
        + q(step_id)
        + " -v status="
        + q(status)
        + " "
        + q(awk)
        + " "
        + q(log_path)
        + " > "
        + q(output_path)
    )
    return machine.execute(command)[0] == 0


def wait_for_rect_manifest_after(log_path, start_line):
    awk = r'''
NR <= start { next }
/^rect manifest end$/ { found = 1 }
END { exit !found }
'''
    machine.execute(
        "for attempt in $(seq 1 30); do "
        + "awk -v start="
        + str(start_line)
        + " "
        + q(awk)
        + " "
        + q(log_path)
        + " && exit 0; "
        + "sleep 0.1; "
        + "done; exit 0"
    )


def initialize_rect_manifest_cursor(log_path, artifact_dir):
    latest = artifact_dir + "/_latest.rects"
    if not extract_rect_manifest(log_path, latest, 0, scenario_id, "_initial", "initial"):
        raise Exception("initial rect manifest missing from " + log_path)
    machine.succeed("test -s " + q(latest))
    return latest


def rect_manifest_count(rects_path):
    output = machine.succeed("awk -F= '/^rect manifest begin count=/ { print $2; exit }' " + q(rects_path)).strip()
    if output == "":
        return None
    return int(output)


def latest_paint_quality_after(log_path, start_line):
    awk = r'''
NR <= start { next }
/^klocc-gui: paint quality/ { rects = $5; line = $0 }
END {
  if (line == "") exit 42
  print rects
  print line
}
'''
    status, output = machine.execute(
        "awk -v start="
        + str(start_line)
        + " "
        + q(awk)
        + " "
        + q(log_path)
    )
    if status != 0:
        return None
    lines = output.splitlines()
    return {"rects": int(lines[0]), "line": lines[1]}


def extract_step_rects(log_path, artifact_dir, stem, step, before_line, previous_rects):
    output = artifact_dir + "/" + stem + ".rects"
    wait_for_rect_manifest_after(log_path, before_line)
    if extract_rect_manifest(log_path, output, before_line, scenario_id, step["id"], "new"):
        machine.succeed("cp " + q(output) + " " + q(artifact_dir + "/_latest.rects"))
        return output

    if previous_rects:
        current_line = log_line_count(log_path)
        latest_paint = latest_paint_quality_after(log_path, before_line)
        previous_count = rect_manifest_count(previous_rects)
        if latest_paint is not None and previous_count is not None and latest_paint["rects"] != previous_count:
            machine.succeed(
                "{ "
                + "printf '%s\n' "
                + q("scenario=" + scenario_id)
                + "; "
                + "printf '%s\n' "
                + q("step=" + step["id"])
                + "; "
                + "printf '%s\n' 'status=paint-only'; "
                + "printf '%s\n' "
                + q("cursor_start=" + str(before_line))
                + "; "
                + "printf '%s\n' "
                + q("cursor_end=" + str(current_line))
                + "; "
                + "printf '%s\n' "
                + q("paint_rects=" + str(latest_paint["rects"]))
                + "; "
                + "printf '%s\n' "
                + q("previous_manifest_rects=" + str(previous_count))
                + "; "
                + "printf '%s\n' "
                + q("previous_manifest=" + previous_rects)
                + "; "
                + "printf '%s\n' "
                + q("paint_quality_line=" + latest_paint["line"])
                + "; "
                + "printf '%s\n' 'no_rect_manifest_after_cursor=true'; "
                + "} > "
                + q(output)
            )
            machine.succeed("test -s " + q(output))
            return previous_rects

        machine.succeed(
            "{ "
            + "printf '%s\n' "
            + q("scenario=" + scenario_id)
            + "; "
            + "printf '%s\n' "
            + q("step=" + step["id"])
            + "; "
            + "printf '%s\n' 'status=reused-unknown'; "
            + "printf '%s\n' "
            + q("cursor_start=" + str(before_line))
            + "; "
            + "printf '%s\n' "
            + q("cursor_end=" + str(current_line))
            + "; "
            + "printf '%s\n' "
            + q("reused_from=" + previous_rects)
            + "; "
            + "printf '%s\n' 'reused_manifest_begin'; "
            + "cat "
            + q(previous_rects)
            + "; "
            + "printf '\n%s\n' 'reused_manifest_end'; "
            + "} > "
            + q(output)
        )
        machine.succeed("test -s " + q(output))
        return previous_rects

    raise Exception("no rect manifest produced for step " + step["id"] + " and no previous manifest exists")


def assert_layout_artifacts(artifact_dir):
    for index, step in enumerate(scenario_steps, start=1):
        stem = step_stem(index, step)
        rects = artifact_dir + "/" + stem + ".rects"
        machine.succeed("test -s " + q(rects))
        machine.succeed("grep -q '^scenario=' " + q(rects))
        machine.succeed("grep -q '^step=' " + q(rects))
        machine.succeed("grep -q '^status=' " + q(rects))
        machine.succeed(
            "grep -Eq '^(rect manifest begin|reused_from=|paint_quality_line=)' "
            + q(rects)
        )


def build_walkthrough_montage(images, output_png, output_stats):
    if not images:
        raise Exception("walkthrough montage requires at least one step")
    vertical_separator = output_png + ".v-separator.png"
    horizontal_separator = output_png + ".h-separator.png"
    machine.succeed("magick -size 10x" + str(screen_height) + " 'xc:#232634' " + q(vertical_separator))
    machine.succeed("magick -size " + str(screen_width * 2 + 10) + "x10 'xc:#232634' " + q(horizontal_separator))
    row_paths = []
    for row_index in range(0, len(images), 2):
        row_images = images[row_index : row_index + 2]
        row_path = output_png + ".row-" + str(row_index // 2) + ".png"
        if len(row_images) == 1:
            machine.succeed("cp " + q(row_images[0]) + " " + q(row_path))
        else:
            machine.succeed(
                "magick "
                + q(row_images[0])
                + " "
                + q(vertical_separator)
                + " "
                + q(row_images[1])
                + " +append "
                + q(row_path)
            )
        row_paths.append(row_path)
    if len(row_paths) == 1:
        machine.succeed("cp " + q(row_paths[0]) + " " + q(output_png))
    else:
        command = "magick " + q(row_paths[0])
        for row_path in row_paths[1:]:
            command += " " + q(horizontal_separator) + " " + q(row_path)
        machine.succeed(command + " -append " + q(output_png))
    machine.succeed("magick identify -format '%[mean] %[standard-deviation] %wx%h' " + q(output_png) + " > " + q(output_stats))
    if scenario_assertions.get("images", {}).get("requireWalkthrough", True):
        assert_nonblank_stats(output_stats)
