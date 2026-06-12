#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import json
import math
import subprocess
from html import escape
from pathlib import Path


def image_uri(path: Path) -> str:
    return "data:image/png;base64," + base64.b64encode(path.read_bytes()).decode("ascii")


def parse_env_dirs(values: list[str]) -> dict[str, Path]:
    result = {}
    for value in values:
        if "=" not in value:
            raise SystemExit(f"--env-dir must be ENV=PATH, got {value!r}")
        env_id, path = value.split("=", 1)
        result[env_id] = Path(path)
    return result


def default_result_dir(env_id: str, scenario_id: str) -> Path:
    return Path(f"/tmp/opencode/klocc-{scenario_id}-{env_id}-result")


def load_scenario(path: Path) -> dict:
    if path.suffix == ".nix":
        try:
            data = subprocess.check_output(["nix", "eval", "--json", "--file", str(path)], text=True)
        except FileNotFoundError as error:
            raise SystemExit("loading a Nix scenario requires `nix` on PATH") from error
        except subprocess.CalledProcessError as error:
            raise SystemExit(f"failed to evaluate Nix scenario {path}: {error}") from error
        return json.loads(data)
    return json.loads(path.read_text())


def load_environments(path: Path) -> list[dict]:
    try:
        data = subprocess.check_output(["nix", "eval", "--json", "--file", str(path), "review"], text=True)
    except FileNotFoundError as error:
        raise SystemExit("loading Nix environment metadata requires `nix` on PATH") from error
    except subprocess.CalledProcessError as error:
        raise SystemExit(f"failed to evaluate Nix environment metadata {path}: {error}") from error
    return json.loads(data)


def ordered_environments(scenario: dict, environments: list[dict], overrides: dict[str, Path]) -> list[tuple[str, dict, Path]]:
    ordered = []
    for env in environments:
        env_id = env["id"]
        root = overrides.get(env_id, default_result_dir(env_id, scenario["id"]))
        ordered.append((env_id, env, root))
    return ordered


def step_stem(index: int, step: dict) -> str:
    return f"{index:02d}-{step['id']}"


def expand_steps(scenario: dict) -> list[dict]:
    steps = []
    for index, step in enumerate(scenario["steps"], start=1):
        stem = step_stem(index, step)
        steps.append({**step, "index": index, "stem": stem, "image": f"{stem}.png", "stats": f"{stem}.stats"})
    return steps


def image_id(env_id: str, step: dict) -> str:
    return f"{env_id}-{step['stem']}"


def card(env_id: str, env: dict, root: Path, step: dict) -> str:
    current_image_id = image_id(env_id, step)
    image = root / step["image"]
    if not image.is_file():
        raise SystemExit(f"missing screenshot for {env_id}:{step['id']}: {image}")
    title = f"{env['short']}-{step['index']} · {step['title']}"
    aria = f"Open {env['label']} {step['title']} full view"
    return f'''<figure class="shot" data-open-image="{escape(current_image_id)}" role="button" tabindex="0" aria-label="{escape(aria)}">
  <figcaption><span>{escape(title)}</span><span>{escape(step['label'])}</span></figcaption>
  <img src="{image_uri(image)}" alt="{escape(aria)}" loading="eager">
</figure>'''


def single(env_id: str, env: dict, root: Path, step: dict) -> str:
    current_image_id = image_id(env_id, step)
    image = root / step["image"]
    title = f"{env['short']}-{step['index']} · {env['label']} · {step['title']}"
    return f'''<section class="view single" id="single-{escape(current_image_id)}" data-single="{escape(current_image_id)}">
  <figure class="solo-shot">
    <figcaption>{escape(title)} · {escape(step['description'])}</figcaption>
    <img src="{image_uri(image)}" alt="{escape(title)}">
  </figure>
</section>'''


def grid_style(rows: int, columns: int) -> str:
    return f"grid-template: repeat({rows}, minmax(0, 1fr)) / repeat({columns}, minmax(0, 1fr));"


def environment_grid_size(count: int) -> tuple[int, int]:
    columns = min(2, max(1, count))
    rows = math.ceil(count / columns)
    return rows, columns


def render_controls(scenario: dict, steps: list[dict], envs: list[tuple[str, dict, Path]]) -> str:
    compare_label = f"Compare {len(envs)}x{len(steps)}"
    env_buttons = "\n".join(
        f'<button class="view-tab" data-view="env-{escape(env_id)}">{escape(env["label"])} {len(steps)}-grid</button>'
        for env_id, env, _root in envs
    )
    image_buttons = []
    for env_id, env, _root in envs:
        for step in steps:
            current_image_id = image_id(env_id, step)
            image_buttons.append(
                f'<button class="image-tab" data-image="{escape(current_image_id)}" title="{escape(env["label"])} · {escape(step["title"])}">{escape(env["short"])}-{escape(str(step["index"]))}</button>'
            )
    image_button_html = "\n      ".join(image_buttons)
    return f'''<nav class="controls" aria-label="View controls">
    <div class="row" role="group" aria-label="Grid views">
      <button class="view-tab active" data-view="compare">{escape(compare_label)}</button>
      {env_buttons}
    </div>
    <div class="row" role="group" aria-label="Individual images">
      {image_button_html}
    </div>
  </nav>'''


def render_main(steps: list[dict], envs: list[tuple[str, dict, Path]]) -> str:
    compare_cards = []
    for step in steps:
        for env_id, env, root in envs:
            compare_cards.append(card(env_id, env, root, step))
    compare_card_html = "\n        ".join(compare_cards)
    compare = f'''<section class="view active grid-view" id="view-compare">
      <div class="grid" style="{grid_style(len(steps), len(envs))}">
        {compare_card_html}
      </div>
    </section>'''

    env_sections = []
    single_sections = []
    for env_id, env, root in envs:
        rows, columns = environment_grid_size(len(steps))
        card_html = "\n        ".join(card(env_id, env, root, step) for step in steps)
        env_sections.append(
            f'''<section class="view grid-view" id="view-env-{escape(env_id)}">
      <div class="grid" style="{grid_style(rows, columns)}">
        {card_html}
      </div>
    </section>'''
        )
        single_sections.extend(single(env_id, env, root, step) for step in steps)

    env_section_html = "\n    ".join(env_sections)
    single_section_html = "\n    ".join(single_sections)
    return f'''<main>
    {compare}
    {env_section_html}
    {single_section_html}
  </main>'''


def render(template: Path, scenario: dict, envs: list[tuple[str, dict, Path]]) -> str:
    steps = expand_steps(scenario)
    html = template.read_text()
    labels = ", ".join(env["label"] for _env_id, env, _root in envs)
    html = html.replace("%%TITLE%%", escape(scenario["title"]))
    html = html.replace("%%SUBTITLE%%", escape(f"{scenario['label']} · {scenario['description']}"))
    html = html.replace("%%META%%", escape(f"compare: {labels}"))
    html = html.replace("%%CONTENT%%", render_controls(scenario, steps, envs) + "\n" + render_main(steps, envs))
    if "%%" in html:
        raise SystemExit("unfilled placeholders remain in review HTML")
    return html


def parse_args() -> argparse.Namespace:
    repo_root = next(parent for parent in Path(__file__).resolve().parents if (parent / "flake.nix").is_file())
    parser = argparse.ArgumentParser(description="Build a klocc GUI screenshot review HTML page.")
    parser.add_argument("--scenario", type=Path, default=repo_root / "nix/snapshot/scenarios/treemap-drilldown.nix")
    parser.add_argument("--environments", type=Path, default=repo_root / "nix/snapshot/environments/default.nix")
    parser.add_argument("--env-dir", action="append", default=[], help="Override environment result directory as ENV=PATH")
    parser.add_argument("--template", type=Path, default=Path(__file__).with_name("wayland-gui-review-template.html"))
    parser.add_argument("--out", type=Path, default=None)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    scenario = load_scenario(args.scenario)
    envs = ordered_environments(scenario, load_environments(args.environments), parse_env_dirs(args.env_dir))
    out = args.out or Path(f"/tmp/opencode/klocc-{scenario['id']}-review.html")
    out.write_text(render(args.template, scenario, envs))
    print(out)


if __name__ == "__main__":
    main()
