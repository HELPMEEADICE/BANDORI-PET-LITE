# BANDORI-PET-REV — Exact Behavioral Spec (extracted from Python source)

Source root: `C:\oss\BANDORI-PET-REV`. Every number/string below is copied verbatim from the files named in each section.
All resolved examples use the **real bundled model** `models/kasumi.zst :: live_default/model.json`
(extracted: 28 motions `angry01, bye01, cry01, eeto01, gattsu01, hukumi01, idle01, idle02, jaan01, kime01, nf01..nf05, niyaniya01, oowarai01, sad01, serious01, shame01, sing01, sleep01, sleep02, smile01..smile03, surprised01, wink01`;
18 expressions `f01..f18`).

---

## 1. `click_motion_presets.py` — the 7 built-in profiles

`BUILTIN_CLICK_MOTION_PROFILES` is a list of dicts `{name, i18n_key, is_builtin, tags}`.
**A preset maps region → ONE tag string.** That single tag is used *twice*: for motion lookup
(`resolve_motion_by_tag`) and for expression lookup (`resolve_expression_by_tag`). There is no
separate motion tag and expression tag in the preset data.

```python
BUILTIN_CLICK_MOTION_PROFILES = [
    {"name": "auto",      "i18n_key": "SettingsWindow.click_motion_profile_preset_auto",      "is_builtin": True, "tags": {}},
    {"name": "genki",     "i18n_key": "SettingsWindow.click_motion_profile_preset_genki",     "is_builtin": True, "tags": {...}},
    {"name": "tsundere",  "i18n_key": "SettingsWindow.click_motion_profile_preset_tsundere",  "is_builtin": True, "tags": {...}},
    {"name": "shy",       "i18n_key": "SettingsWindow.click_motion_profile_preset_shy",       "is_builtin": True, "tags": {...}},
    {"name": "cool",      "i18n_key": "SettingsWindow.click_motion_profile_preset_cool",      "is_builtin": True, "tags": {...}},
    {"name": "surprised", "i18n_key": "SettingsWindow.click_motion_profile_preset_surprised", "is_builtin": True, "tags": {...}},
    {"name": "random",    "i18n_key": "SettingsWindow.click_motion_profile_preset_random",    "is_builtin": True, "tags": {"__mode__": "random"}},
]
BUILTIN_PROFILE_NAMES = {p["name"] for p in BUILTIN_CLICK_MOTION_PROFILES}
```

### 1.1 Full region → tag table (exact strings)

Region order is fixed by `CLICK_MOTION_REGIONS`:
`head, upper_body_left, upper_body_center, upper_body_right, lower_body_left, lower_body_center, lower_body_right`.

| region | auto | genki | tsundere | shy | cool | surprised | random |
|---|---|---|---|---|---|---|---|
| head | *(none)* | `smile` | `shame` | `shame` | `serious` | `surprised` | `__mode__:random` |
| upper_body_left | *(none)* | `nf_left` | `pui` | `nnf_left` | `kime` | `surprised` | `__mode__:random` |
| upper_body_center | *(none)* | `smile` | `angry` | `shame` | `serious` | `surprised` | `__mode__:random` |
| upper_body_right | *(none)* | `nf_right` | `pui` | `nnf_right` | `kime` | `surprised` | `__mode__:random` |
| lower_body_left | *(none)* | `smile` | `shame` | `sad` | `serious` | `surprised` | `__mode__:random` |
| lower_body_center | *(none)* | `kime` | `angry` | `shame` | `kime` | `surprised` | `__mode__:random` |
| lower_body_right | *(none)* | `smile` | `serious` | `sad` | `serious` | `surprised` | `__mode__:random` |

Verbatim source of the non-trivial ones:

```python
    {
        "name": "genki",
        "tags": {
            "head": "smile",
            "upper_body_left": "nf_left",
            "upper_body_center": "smile",
            "upper_body_right": "nf_right",
            "lower_body_left": "smile",
            "lower_body_center": "kime",
            "lower_body_right": "smile",
        },
    },
    {
        "name": "tsundere",
        "tags": {
            "head": "shame",
            "upper_body_left": "pui",
            "upper_body_center": "angry",
            "upper_body_right": "pui",
            "lower_body_left": "shame",
            "lower_body_center": "angry",
            "lower_body_right": "serious",
        },
    },
    {
        "name": "shy",
        "tags": {
            "head": "shame",
            "upper_body_left": "nnf_left",
            "upper_body_center": "shame",
            "upper_body_right": "nnf_right",
            "lower_body_left": "sad",
            "lower_body_center": "shame",
            "lower_body_right": "sad",
        },
    },
    {
        "name": "cool",
        "tags": {
            "head": "serious",
            "upper_body_left": "kime",
            "upper_body_center": "serious",
            "upper_body_right": "kime",
            "lower_body_left": "serious",
            "lower_body_center": "kime",
            "lower_body_right": "serious",
        },
    },
    {
        "name": "surprised",
        "tags": {
            "head": "surprised", "upper_body_left": "surprised", "upper_body_center": "surprised",
            "upper_body_right": "surprised", "lower_body_left": "surprised",
            "lower_body_center": "surprised", "lower_body_right": "surprised",
        },
    },
```

### 1.2 Expansion of a preset into stored actions

```python
def resolve_preset_to_actions(preset, motion_names, expression_names, char_name=""):
    tags = preset.get("tags", {})
    if tags.get("__mode__") == "random":
        return {region: {"motion": "__random__", "expression": ""} for region in CLICK_MOTION_REGIONS}
    if not tags:
        return {region: {"motion": CLICK_MOTION_AUTO, "expression": ""} for region in CLICK_MOTION_REGIONS}
    actions = {}
    for region in CLICK_MOTION_REGIONS:
        tag = tags.get(region, "")
        if not tag:
            actions[region] = {"motion": CLICK_MOTION_AUTO, "expression": ""}
            continue
        motion = resolve_motion_by_tag(tag, motion_names, char_name)
        expression = resolve_expression_by_tag(tag, expression_names, char_name)
        actions[region] = {"motion": motion or CLICK_MOTION_AUTO, "expression": expression}
    return actions
```

Key semantics for Rust:
* `CLICK_MOTION_AUTO = ""`, `CLICK_MOTION_RANDOM = "__random__"`, `CLICK_MOTION_NONE = "__none__"`.
* A preset is resolved **once, at settings-apply time**, against that model's real motion/expression names,
  and the resulting `click_motion_actions` map is persisted into the model entry (see §7). The pet window
  later just reads the stored strings; it never re-resolves preset tags.
* `motion` falls back to `""` when no motion name matches; `expression` is whatever the expression resolver
  returns (may be `f01`-style first-name fallback, see §4).

### 1.3 Resolved output for the real kasumi/live_default model (`char_name="kasumi"`)

Motion column = the **candidate pool** (the un-deduplicated `matches` list; `random.choice` picks one at
save time). Empty pool ⇒ stored `motion = ""`.

| preset | region | tag | expr candidates (`_EXPRESSION_TAG_MAP`) | expr result | motion pool |
|---|---|---|---|---|---|
| genki | head | smile | `["smile"]` | `f01` | `smile01, smile02, smile03` |
| genki | upper_body_left | nf_left | `["smile","idle"]` | `f01` | *(empty)* |
| genki | upper_body_center | smile | `["smile"]` | `f01` | `smile01..03` |
| genki | upper_body_right | nf_right | `["smile","idle"]` | `f01` | *(empty)* |
| genki | lower_body_left | smile | `["smile"]` | `f01` | `smile01..03` |
| genki | lower_body_center | kime | `["kime"]` | `f01` | `kime01` |
| genki | lower_body_right | smile | `["smile"]` | `f01` | `smile01..03` |
| tsundere | head | shame | `["shame"]` | `f01` | `shame01` |
| tsundere | upper_body_left | pui | `["pui"]` | `f01` | *(empty)* |
| tsundere | upper_body_center | angry | `["angry"]` | `f01` | `angry01` |
| tsundere | upper_body_right | pui | `["pui"]` | `f01` | *(empty)* |
| tsundere | lower_body_left | shame | `["shame"]` | `f01` | `shame01` |
| tsundere | lower_body_center | angry | `["angry"]` | `f01` | `angry01` |
| tsundere | lower_body_right | serious | `["serious"]` | `f01` | `serious01` |
| shy | head | shame | `["shame"]` | `f01` | `shame01` |
| shy | upper_body_left | nnf_left | `["smile","idle"]` | `f01` | *(empty)* |
| shy | upper_body_center | shame | `["shame"]` | `f01` | `shame01` |
| shy | upper_body_right | nnf_right | `["smile","idle"]` | `f01` | *(empty)* |
| shy | lower_body_left | sad | `["sad","cry"]` | `f01` | `sad01` |
| shy | lower_body_center | shame | `["shame"]` | `f01` | `shame01` |
| shy | lower_body_right | sad | `["sad","cry"]` | `f01` | `sad01` |
| cool | head/upper_body_center/lower_body_left/lower_body_right | serious | `["serious"]` | `f01` | `serious01` |
| cool | upper_body_left/upper_body_right/lower_body_center | kime | `["kime"]` | `f01` | `kime01` |
| surprised | all 7 | surprised | `["surprised"]` | `f01` | `surprised01` |

Why every expression resolves to `f01`: this pack's expression names are `f01..f18`, so no tag matches and the
final fallback returns `expression_names[0]`. This is *real behavior*, not an artifact — reproduce it.

### 1.4 Display strings (Chinese, used when no tr function is injected)

```python
_PRESET_DISPLAY_DEFAULTS = {
    "auto": ("自动选择", "系统根据点击区域自动匹配合适的动作"),
    "genki": ("元气开朗", "充满活力的笑脸与积极反应"),
    "tsundere": ("傲娇", "嘴上不饶人但内心害羞的经典傲娇反应"),
    "shy": ("害羞内向", "容易害羞、轻微社恐的可爱反应"),
    "cool": ("冷静理智", "沉着冷静的认真反应"),
    "surprised": ("惊讶意外", "对各种触碰都感到惊讶的反应"),
    "random": ("随机", "每次点击随机播放动作"),
}

def preset_display_name(preset, tr_func=None) -> str:
    i18n_key = preset.get("i18n_key", "")
    if tr_func and i18n_key:
        return tr_func(i18n_key)
    name = preset["name"]
    return _PRESET_DISPLAY_DEFAULTS.get(name, (name, ""))[0]

def preset_combo_label(preset, tr_func=None) -> str:
    return f"[{preset_display_name(preset, tr_func)}]"
```

---

## 2. Region classification — `live2d_click_actions.click_motion_region_for_point`

Constants (`live2d_click_actions.py`):

```python
CLICK_MOTION_REGIONS = (
    "head", "upper_body_left", "upper_body_center", "upper_body_right",
    "lower_body_left", "lower_body_center", "lower_body_right",
)
CLICK_MOTION_AUTO = ""
CLICK_MOTION_RANDOM = "__random__"
CLICK_MOTION_NONE = "__none__"
CLICK_MOTION_SPECIAL_VALUES = {CLICK_MOTION_AUTO, CLICK_MOTION_RANDOM, CLICK_MOTION_NONE}
```

Full function (verbatim):

```python
def click_motion_region_for_point(x, y, width, height, area_name="", area_bounds=None) -> str:
    width = max(1, int(width or 1))
    height = max(1, int(height or 1))
    area = str(area_name or "").strip().lower()

    if area in {"head", "face"}:
        return "head"

    if area_bounds:
        try:
            min_x, max_x, min_y, max_y = (float(v) for v in area_bounds)
            bounds_w = max_x - min_x
            bounds_h = max_y - min_y
            if bounds_w > 1 and bounds_h > 1:
                x_ratio = (float(x) - min_x) / bounds_w
                y_ratio = (float(y) - min_y) / bounds_h
            else:
                x_ratio = float(x) / width
                y_ratio = float(y) / height
        except (TypeError, ValueError):
            x_ratio = float(x) / width
            y_ratio = float(y) / height
    else:
        x_ratio = float(x) / width
        y_ratio = float(y) / height

    x_ratio = max(0.0, min(1.0, x_ratio))
    y_ratio = max(0.0, min(1.0, y_ratio))

    if area in {"", "body", "hit"} and y_ratio < 0.38:
        return "head"

    vertical = "upper_body" if y_ratio < 0.64 else "lower_body"
    if x_ratio < 0.38:
        horizontal = "left"
    elif x_ratio > 0.62:
        horizontal = "right"
    else:
        horizontal = "center"
    return f"{vertical}_{horizontal}"
```

Decision order for Rust:
1. `area in {"head","face"}` → `head` (unconditional, before any coordinate math).
2. Normalize: if `area_bounds=(min_x,max_x,min_y,max_y)` and `max_x-min_x > 1` **and** `max_y-min_y > 1` → ratios are relative to the hit-area bbox; otherwise relative to widget size `width/height`. Ratios clamped to `[0,1]`.
3. `area in {"","body","hit"}` and `y_ratio < 0.38` → `head`.
4. vertical split at **0.64** (upper if `< 0.64`, else lower).
5. horizontal: `< 0.38` left, `> 0.62` right, else center (so `0.38 ≤ x ≤ 0.62` is center; note `<0.38` / `>0.62` are strict, `0.62` itself is center).

Hit-area name mapping (what `area_name` actually is) — `live2d_widget_base.py`:

```python
def hit_area_name_at(self, x, y) -> str:
    if not self._model: return ""
    return self._custom_hit_area_name_at(x, y) or self._sdk_hit_area_name_at(x, y)

def _sdk_hit_area_name_at(self, x, y) -> str:
    if not self._has_sdk_hit_areas(): return ""
    return (self._model.HitTest("", x, y) or "").strip().lower()

def _custom_hit_area_name_at(self, x, y) -> str:
    if self._custom_hit_areas is None: return ""
    return self._custom_hit_areas.hit_test_name(x, y).strip().lower()

def _has_sdk_hit_areas(self) -> bool:
    return self._model and self._model.modelSetting.getHitAreaNum() > 0
```

So `area_name` arrives already lower-cased; custom (Lua) hit areas take precedence over the Cubism SDK hit areas.
`area_bounds` comes from `PetWindow._click_motion_area_bounds`:

```python
def _click_motion_area_bounds(self, area_name):
    area_name = (area_name or "").strip().lower()
    if area_name in {"head", "face"}:
        return (self._live2d_widget.hit_area_bounds(area_name)
                or self._live2d_widget.hit_area_union_bounds())
    if area_name in {"body", "hit", ""}:
        return (self._live2d_widget.hit_area_bounds("body")
                or self._live2d_widget.hit_area_union_bounds())
    return (self._live2d_widget.hit_area_bounds(area_name)
            or self._live2d_widget.hit_area_union_bounds())
```

---

## 3. Name matching — exact 4-check algorithm

Two identical implementations exist: `click_motion_presets.resolve_motion_by_tag(tag, motion_names, char_name)`
and `PetWindow._resolve_motion_tag(tag, motion_names)` (the latter uses `self._current_char`). Verbatim:

```python
def resolve_motion_by_tag(tag: str, motion_names: list[str], char_name: str = "") -> str:
    tag_low = tag.lower()
    char_lower = (char_name or "").lower()
    candidates = [tag_low]
    if tag_low == "thinking":
        candidates.extend(["nf", "nnf", "eeto", "odoodo"])

    matches = []
    for candidate in candidates:
        candidate_prefix = f"{char_lower}_{candidate}" if char_lower else candidate
        for motion_name in motion_names:
            motion_low = str(motion_name).lower()
            if motion_low == candidate or motion_low.startswith(candidate):
                matches.append(str(motion_name))
            elif motion_low == candidate_prefix or motion_low.startswith(candidate_prefix):
                matches.append(str(motion_name))
            elif re.search(rf"(^|[_\-]){re.escape(candidate)}($|[_\-]?\d)", motion_low):
                matches.append(str(motion_name))
    return random.choice(matches) if matches else ""
```

Check order per *(candidate, name)* pair — first hit wins, `elif` chain:

1. **exact-or-prefix on bare tag**: `motion_low == candidate or motion_low.startswith(candidate)`
   (so `smile` matches `smile01`, `smile02`, `smile03`; `nf` matches `nf01..nf05`).
2. **exact-or-prefix on `<char>_<tag>`**: `motion_low == f"{char_lower}_{candidate}" or motion_low.startswith(f"{char_lower}_{candidate}")`.
3. **regex**: `rf"(^|[_\-]){re.escape(candidate)}($|[_\-]?\d)"` applied to the lower-cased name
   (anchored at a word boundary: start or `_`/`-` before; end or optional `_`/`-` then a digit after).
   This catches `mtn_smile01`, `mtn-smile`, `smile_01`, `smile1`, etc.
4. (implicit) no match → not appended.

Notes for a faithful port:
* `candidates` is `[tag]` plus, **only for the literal tag `thinking`**, `nf, nnf, eeto, odoodo` appended in that order.
* `matches` is **not de-duplicated** and preserves outer-candidate/name iteration order. When `char_name` is empty,
  check 1 and check 2 are the same predicate, so every matching name is appended **twice** (uniform duplication —
  the distribution over distinct names is unchanged, but a faithful port should replicate the list).
* Selection is uniform over the list: `random.choice(matches)`; empty list ⇒ `""` (which callers turn into `CLICK_MOTION_AUTO`).
* Motion names supplied by `ModelManager.get_motion_names()` are **sorted** (`sorted(str(name) for name in motions if name)`).

Expression resolver — **deterministic, first match wins, no random**:

```python
def resolve_expression_by_tag(tag: str, expression_names: list[str], char_name: str = "") -> str:
    tag_low = tag.lower()
    char_lower = (char_name or "").lower()
    target_tags = _EXPRESSION_TAG_MAP.get(tag_low, [tag_low])

    for expr_tag in target_tags:
        candidates = [expr_tag]
        if expr_tag == "default":
            candidates.append("idle")
        for candidate in candidates:
            candidate_prefix = f"{char_lower}_{candidate}" if char_lower else candidate
            for expr_name in expression_names:
                expr_low = str(expr_name).lower()
                if expr_low == candidate or expr_low.startswith(candidate):
                    return str(expr_name)
                elif expr_low == candidate_prefix or expr_low.startswith(candidate_prefix):
                    return str(expr_name)
                elif re.search(rf"(^|[_\-]){re.escape(candidate)}($|[_\-]?\d)", expr_low):
                    return str(expr_name)

    for fallback_tag in ("default", "idle", "smile"):
        for expr_name in expression_names:
            expr_low = str(expr_name).lower()
            if fallback_tag in expr_low:
                return str(expr_name)

    return expression_names[0] if expression_names else ""
```

### 3.1 Special values `__random__` / `__none__` / `""`

Handling is at *click* time in `PetWindow._trigger_click_motion` (never inside the resolvers):

```python
    feedback = self._configured_click_motion_feedback(region)
    configured_motion = feedback.get("motion", "")
    configured_expression = feedback.get("expression", "")
    if configured_motion == CLICK_MOTION_NONE:
        return                                   # "__none__": no motion, no expression, no poke
    if configured_motion == CLICK_MOTION_RANDOM:
        self._start_click_motion("", configured_expression)   # "__random__": any non-idle prewarmed motion
        return
    if configured_motion:
        self._start_click_motion(configured_motion, configured_expression)
        return
    motion = self._choose_click_action_motion(region)         # "" = AUTO → bucket lookup (§3.2)
    if motion:
        self._start_click_motion(motion, configured_expression)
    else:
        self._start_click_motion("", configured_expression)
```

`""` (= `CLICK_MOTION_AUTO`) ⇒ region bucket lookup. `"__random__"` ⇒ `_start_click_motion("")`, and inside
`_start_click_motion` an empty motion name means: use `_choose_prewarmed_click_motion()` (random prewarmed
non-`idle*`/`sys-*` motion); if none, random choice over all names not starting with `idle`/`sys-`; if none,
random over all names.

`normalize_click_motion_actions` validates against the model's real name sets but always keeps the three
special values:

```python
        if not motion and not expression:
            continue
        if valid_motion_set is not None and motion not in CLICK_MOTION_SPECIAL_VALUES and motion not in valid_motion_set:
            motion = ""
        if valid_expression_set is not None and expression not in valid_expression_set:
            expression = ""
```

### 3.2 AUTO buckets (`_AUTO_BUCKETS`) and click-time choice

```python
_AUTO_BUCKETS = {
    "head":               (("surprised", "shame", "pui", "smile"),      ("kandou", "kime", "nf")),
    "upper_body_left":    (("nf_left", "nnf_left"),                     ("shame", "surprised", "smile")),
    "upper_body_center":  (("smile", "kime", "surprised", "shame"),     ("angry", "pui", "nf")),
    "upper_body_right":   (("nf_right", "nnf_right"),                   ("shame", "surprised", "smile")),
    "lower_body_left":    (("nf_left", "nnf_left"),                     ("surprised", "sad", "smile")),
    "lower_body_center":  (("shame", "surprised", "angry"),             ("smile", "kime")),
    "lower_body_right":   (("nf_right", "nnf_right"),                   ("surprised", "sad", "smile")),
}
def click_motion_auto_buckets(region: str):
    return _AUTO_BUCKETS.get(region, _AUTO_BUCKETS["upper_body_center"])
```

```python
def _choose_click_action_motion(self, region: str) -> str:
    motion_names = self._current_motion_names()
    if not motion_names:
        return ""
    for bucket in click_motion_auto_buckets(region):
        available = [motion for tag in bucket
                     if (motion := self._resolve_motion_tag(tag, motion_names))]
        if available:
            warmed = [motion for motion in available if motion in self._live2d_prewarmed_motions]
            return random.choice(warmed or available)
    non_idle = [name for name in motion_names
                if not str(name).lower().startswith(("idle", "sys-"))]
    warmed = [name for name in non_idle if name in self._live2d_prewarmed_motions]
    return random.choice(warmed or non_idle) if non_idle else ""
```

Bucket semantics: try bucket 1 tags **in order**, keep only tags that resolve to a motion, then pick uniformly
(preferring prewarmed). If bucket 1 yields nothing, try bucket 2 the same way. If both empty, fall back to
random non-idle, preferring prewarmed.

Resolved for kasumi/live_default (pools shown per tag):

| region | bucket 1 pools | bucket 2 pools |
|---|---|---|
| head | surprised→`surprised01`, shame→`shame01`, pui→∅, smile→`smile01..03` | kandou→∅, kime→`kime01`, nf→`nf01..05` |
| upper_body_left | nf_left→∅, nnf_left→∅ | shame→`shame01`, surprised→`surprised01`, smile→`smile01..03` |
| upper_body_center | smile→`smile01..03`, kime→`kime01`, surprised→`surprised01`, shame→`shame01` | angry→`angry01`, pui→∅, nf→`nf01..05` |
| upper_body_right | nf_right→∅, nnf_right→∅ | shame→`shame01`, surprised→`surprised01`, smile→`smile01..03` |
| lower_body_left | nf_left→∅, nnf_left→∅ | surprised→`surprised01`, sad→`sad01`, smile→`smile01..03` |
| lower_body_center | shame→`shame01`, surprised→`surprised01`, angry→`angry01` | smile→`smile01..03`, kime→`kime01` |
| lower_body_right | nf_right→∅, nnf_right→∅ | surprised→`surprised01`, sad→`sad01`, smile→`smile01..03` |

### 3.3 Click / double-click / right-click plumbing (`live2d_widget_base.py`)

```python
        # mousePressEvent (left button)
        pos = event.scenePosition()
        self._pressed_on_model = self._is_model_hit_at(pos.x(), pos.y())
        if self._pressed_on_model:
            event.accept()
        if self._drag_locked:
            return
        ...
        # mouseReleaseEvent
        should_click = (
            self._pressed_on_model
            and not self._drag_moved
            and self._click_callback
        )
        self._pressed_on_model = False
        self._finish_window_drag()
        if should_click:
            self._click_callback(x, y, self.hit_area_name_at(x, y))   # → PetWindow._on_click
        # mouseDoubleClickEvent (left)
        if self._double_click_callback and self._is_model_hit_at(x, y):
            self._pressed_on_model = False
            self._finish_window_drag()
            self._drag_moved = False
            self._double_click_callback(x, y, self.hit_area_name_at(x, y))  # → _on_live2d_double_click
```

Drag-vs-click discrimination: a press that never exceeds `total_dx*total_dx + total_dy*total_dy >= 16`
(4 px) is still a click (`_drag_moved` stays False).

Double click path (`PetWindow._on_live2d_double_click`):
close radial menu if visible → `_can_handle_live2d_user_poke()` gate (compact AI window enabled, OR chat
process running, OR group has >1 character) → `_trigger_user_poke_feedback()` → `_play_emotion_window_feedback("shake", 72)`
→ `publish_user_poke(char, source="live2d")`.

`_trigger_click_motion` side effects / timers after a motion actually starts:

```python
        if started:
            if expression:
                QTimer.singleShot(80, lambda t=..., e=expression: self._set_click_expression_if_current(t, e))
            QTimer.singleShot(9000, lambda t=token: self._clear_motion_if_current(t))
            QTimer.singleShot(3200, lambda t=token: self._restore_default_if_finished(t))
        elif expression:
            QTimer.singleShot(5000, lambda t=...: self._restore_default_expression_if_current(t))
```

Expression hold bookkeeping: `self._click_expression_hold_until = max(self._click_expression_hold_until, time.monotonic() + 5.0)`, revert scheduled at **5000 ms**.

---

## 4. Tag → expression candidate table (complete) + fallback chain

```python
_EXPRESSION_TAG_MAP = {
    "surprised": ["surprised"],
    "shame": ["shame"],
    "smile": ["smile"],
    "angry": ["angry"],
    "sad": ["sad", "cry"],
    "serious": ["serious"],
    "kime": ["kime"],
    "pui": ["pui"],
    "nf": ["smile", "idle"],
    "nf_left": ["smile", "idle"],
    "nf_right": ["smile", "idle"],
    "nnf": ["smile", "idle"],
    "nnf_left": ["smile", "idle"],
    "nnf_right": ["smile", "idle"],
    "kandou": ["surprised", "kime"],
    "cry": ["cry", "sad"],
    "idle": ["idle", "default"],
    "scared": ["surprised"],
    "thinking": ["serious", "idle"],
    "stare": ["serious", "kime"],
    "bye": ["smile", "idle"],
    "odoodo": ["shame", "serious"],
    "wink": ["smile"],
    "nod": ["smile", "idle"],
}
```

That is the complete map (24 entries). Any tag not present ⇒ `target_tags = [tag_lower]` (identity) — e.g. `genki`'s
`nf_left` is in the map, but a custom tag `"banana"` would look for expression named `banana`.

Fallback chain when **no** candidate matches, in this exact order:
1. `for fallback_tag in ("default", "idle", "smile"):` — first expression whose lower-cased name **contains** that
   substring (`if fallback_tag in expr_low`), in expression-name iteration order.
2. `return expression_names[0] if expression_names else ""` — first name in the (sorted) list, e.g. `f01`.
3. No expressions at all ⇒ `""` (and `_set_click_expression_if_current` would no-op anyway).

Extra: inside the loop, candidate `"default"` is expanded to `["default", "idle"]`:

```python
        candidates = [expr_tag]
        if expr_tag == "default":
            candidates.append("idle")
```

### 4.1 Second, different resolver used by emotion/idle paths

`PetWindow._find_expression_tag(tag)` (used by `_apply_emotion_expression` and `_choose_context_idle_expression`)
is a *different*, simpler algorithm — implement both if you port those features:

```python
def _find_expression_tag(self, tag: str) -> str:
    if not tag: return ""
    names = list(model.expressions.keys())
    char_prefix = (self._current_char or "").lower()
    for name in names:
        name_low = str(name).lower()
        name_base = os.path.splitext(name_low)[0]
        if name_low == tag or name_base == tag:            return name      # exact / stem
        if char_prefix and name_base == f"{char_prefix}_{tag}": return name  # char prefix (exact)
    for name in names:
        name_base = os.path.splitext(str(name).lower())[0]
        if name_base.endswith(f"_{tag}") or name_base.startswith(f"{tag}"): return name
    return ""
```

---

## 5. Emotion window animation (`emotion_behavior.py` + `PetWindow._play_emotion_window_feedback`)

### 5.1 Emotion tables

```python
ACTION_EMOTION = {
    "smile": "happy", "f": "happy", "wink": "happy",
    "gattsu": "excited", "jaan": "excited", "oowarai": "excited",
    "kandou": "soft", "ando": "soft",
    "sad": "sad", "cry": "sad",
    "angry": "angry", "pui": "angry",
    "kuyasii": "annoyed", "sigh": "annoyed",
    "thinking": "thoughtful", "nf": "thoughtful", "nnf": "thoughtful",
    "eeto": "thoughtful", "odoodo": "thoughtful", "mitore": "thoughtful",
    "shame": "shy",
    "surprised": "surprised", "scared": "surprised", "awate": "surprised",
    "sleep": "tired", "akubi": "tired",
}

EMOTION_BEHAVIOR = {
    "happy":      {"expressions": ("smile", "f", "wink"),        "motions": ("smile", "gattsu", "jaan"),            "window": "hop",     "tts_rate": 1.06},
    "excited":    {"expressions": ("smile", "surprised"),        "motions": ("gattsu", "jaan", "smile"),            "window": "hop",     "tts_rate": 1.12},
    "soft":       {"expressions": ("smile", "default"),          "motions": ("kandou", "ando", "smile"),            "window": "settle",  "tts_rate": 0.96},
    "shy":        {"expressions": ("shame", "smile"),            "motions": ("shame", "odoodo", "eeto"),            "window": "back",    "tts_rate": 0.92},
    "angry":      {"expressions": ("angry", "serious"),          "motions": ("angry", "pui", "kuyasii"),            "window": "forward", "tts_rate": 1.10},
    "annoyed":    {"expressions": ("angry", "serious", "sad"),   "motions": ("pui", "sigh", "angry"),               "window": "wobble",  "tts_rate": 1.04},
    "sad":        {"expressions": ("sad", "cry"),                "motions": ("sad", "cry", "sigh"),                 "window": "settle",  "tts_rate": 0.88},
    "hurt":       {"expressions": ("sad", "cry"),                "motions": ("sad", "cry", "sigh"),                 "window": "back",    "tts_rate": 0.86},
    "concerned":  {"expressions": ("sad", "serious", "default"), "motions": ("thinking", "eeto", "nf"),             "window": "settle",  "tts_rate": 0.94},
    "thoughtful": {"expressions": ("default", "serious"),        "motions": ("thinking", "nf", "nnf", "eeto", "odoodo"), "window": "",   "tts_rate": 0.98},
    "surprised":  {"expressions": ("surprised", "scared"),       "motions": ("surprised", "awate", "scared"),       "window": "shake",   "tts_rate": 1.08},
    "tired":      {"expressions": ("sleep", "sad", "default"),   "motions": ("sleep", "akubi", "sigh"),             "window": "settle",  "tts_rate": 0.84},
    "calm":       {"expressions": ("default",),                  "motions": (),                                     "window": "",        "tts_rate": 1.0},
}
```

### 5.2 Intensity computation (from tags/actions/text)

```python
def infer_emotion_behavior(text, actions=None) -> dict:
    text = str(text or "")
    normalized_actions = [str(action or "").strip().lower().strip("[]")
                          for action in (actions or []) if str(action or "").strip()]
    emotion = _emotion_from_actions(normalized_actions) or _emotion_from_text(text)
    if not emotion:
        return {}
    intensity = _emotion_intensity(text, normalized_actions, emotion)
    base = EMOTION_BEHAVIOR.get(emotion, EMOTION_BEHAVIOR["calm"])
    return {
        "emotion": emotion,
        "intensity": intensity,
        "expression_tags": list(base.get("expressions", ())),
        "motion_tags": list(base.get("motions", ())),
        "window": base.get("window", ""),
        "tts_rate": _scaled_tts_rate(float(base.get("tts_rate", 1.0)), intensity),
        "source_actions": normalized_actions,
    }

def _emotion_intensity(text, actions, emotion) -> int:
    intensity = 56 if emotion in {"calm", "thoughtful", "soft"} else 64
    if actions:
        intensity += 10
    if _INTENSE_PUNCT_RE.search(text):
        intensity += 12
    if any(mark in text for mark in ("...", "……")) and emotion in {"sad", "hurt", "shy", "tired"}:
        intensity += 8
    if len(text) <= 12 and emotion in {"shy", "surprised", "angry"}:
        intensity += 5
    return max(20, min(100, intensity))

_INTENSE_PUNCT_RE = re.compile(r"[!！?？]{2,}")

def _scaled_tts_rate(base_rate: float, intensity: int) -> float:
    delta = base_rate - 1.0
    scale = max(0.45, min(1.0, intensity / 82.0))
    return round(max(0.75, min(1.25, 1.0 + delta * scale)), 3)
```

Emotion from text: `_EMOTION_KEYWORDS` scored by counting contained terms (Chinese keyword tuple per emotion, first
max wins); emotion from actions: last-to-first over actions, exact key or key before the last `.` (`action.rsplit(".", 1)[0]`).

```python
_EMOTION_KEYWORDS = (
    ("shy",      ("害羞", "脸红", "不好意思", "羞", "欸嘿", "诶嘿")),
    ("angry",    ("生气", "气死", "笨蛋", "不许", "哼", "过分")),
    ("annoyed",  ("烦", "讨厌", "真是的", "没办法", "无语")),
    ("sad",      ("难过", "伤心", "哭", "呜", "低落", "寂寞")),
    ("hurt",     ("受伤", "心痛", "委屈", "失望")),
    ("concerned",("担心", "没事吧", "还好吗", "小心", "注意身体")),
    ("surprised",("惊讶", "吓", "欸", "诶", "什么", "不会吧", "真的假的")),
    ("excited",  ("太棒", "超开心", "最棒", "冲呀", "好耶", "哇")),
    ("happy",    ("开心", "高兴", "喜欢", "可爱", "谢谢", "真好", "太好", "加油", "嘿嘿", "哈哈")),
    ("thoughtful",("想想", "思考", "也许", "可能", "让我想")),
    ("tired",    ("困", "累", "晚安", "睡", "哈欠")),
    ("soft",     ("安心", "温柔", "没关系", "放心", "抱抱")),
)
```

Runtime clamps: `_handle_emotion_behavior` uses `intensity = max(20, min(100, int(event.get("intensity", 60))))`
(default **60**, not 56/64 — the default applies only when the event omits intensity).

### 5.3 The 6 window motions (verbatim)

```python
    def _play_emotion_window_feedback(self, kind: str, intensity: int):
        if kind not in {"back", "forward", "hop", "shake", "wobble", "settle"}:
            return
        current = self.pos()
        if kind in {"forward", "back"} and self._live2d_widget._drag_locked:
            return

        animation = getattr(self, "_emotion_window_anim", None)
        if animation is not None:
            try: animation.stop()
            except RuntimeError: pass

        distance = max(8, min(42, int(10 + intensity * 0.32)))
        target = current
        keyframes: list[tuple[float, QPoint]] = []
        duration = 360

        if kind in {"forward", "back"}:
            dx, dy = self._emotion_cursor_direction()
            if kind == "back":
                dx, dy = -dx, -dy
            target = QPoint(current.x() + int(round(dx * distance)), current.y() + int(round(dy * distance)))
            target = self._constrained_emotion_point(target)
            duration = 300
        elif kind == "hop":
            lift = max(10, min(34, int(8 + intensity * 0.24)))
            keyframes = [
                (0.0, current),
                (0.38, self._constrained_emotion_point(QPoint(current.x(), current.y() - lift))),
                (0.72, self._constrained_emotion_point(QPoint(current.x(), current.y() + max(2, lift // 5)))),
                (1.0, current),
            ]
            duration = 420
        elif kind == "shake":
            amp = max(8, min(26, int(6 + intensity * 0.2)))
            keyframes = [
                (0.0, current),
                (0.18, self._constrained_emotion_point(QPoint(current.x() - amp, current.y()))),
                (0.36, self._constrained_emotion_point(QPoint(current.x() + amp, current.y()))),
                (0.55, self._constrained_emotion_point(QPoint(current.x() - amp // 2, current.y()))),
                (0.74, self._constrained_emotion_point(QPoint(current.x() + amp // 2, current.y()))),
                (1.0, current),
            ]
            duration = 360
        elif kind == "wobble":
            amp = max(5, min(16, int(4 + intensity * 0.12)))
            keyframes = [
                (0.0, current),
                (0.25, self._constrained_emotion_point(QPoint(current.x() + amp, current.y() + 2))),
                (0.50, self._constrained_emotion_point(QPoint(current.x() - amp, current.y() - 1))),
                (0.75, self._constrained_emotion_point(QPoint(current.x() + amp // 2, current.y()))),
                (1.0, current),
            ]
            duration = 420
        elif kind == "settle":
            drop = max(3, min(10, int(2 + intensity * 0.08)))
            keyframes = [
                (0.0, current),
                (0.45, self._constrained_emotion_point(QPoint(current.x(), current.y() + drop))),
                (1.0, current),
            ]
            duration = 340

        anim = QVariantAnimation(self)
        anim.setDuration(duration)
        anim.setEasingCurve(QEasingCurve.Type.OutCubic)
        self._emotion_window_anim = anim
        self._emotion_window_animating = True
        if keyframes:
            for step, point in keyframes:
                anim.setKeyValueAt(step, point)
        else:
            anim.setStartValue(current)
            anim.setEndValue(target)
        anim.valueChanged.connect(self._move_emotion_window)
        anim.finished.connect(self._finish_emotion_window_feedback)
        anim.start()
```

**Easing formula**: the animation is a `QVariantAnimation` over `QPoint` keyframes with
`QEasingCurve.Type.OutCubic` — i.e. per-segment progress `p = 1 - (1-t)^3` applied to the vector interpolation
between consecutive keyframes (Qt interpolates QPoint linearly over `QVariantAnimation::Interpolated`).
For `hop`, the up-leg and down-leg each get their own OutCubic remap of the sub-interval.
No easing is applied to `forward`/`back` keyframe indices beyond OutCubic over the single start→end segment.

Summarised parameters:

| kind | formula | clamp | keyframes (t, offset from current) | duration |
|---|---|---|---|---|
| `forward` | `dx,dy` = unit vector from window center to cursor; `distance as below` | | start → `(+dx*D, +dy*D)` | 300 ms |
| `back` | same with `dx,dy` negated | | start → `(-dx*D, -dy*D)` | 300 ms |
| `hop` | `lift = clamp(int(8 + I*0.24), 10, 34)` | 10..34 | 0.00 (0,0); 0.38 (0,−lift); 0.72 (0,+max(2,lift//5)); 1.00 (0,0) | 420 ms |
| `shake` | `amp = clamp(int(6 + I*0.20), 8, 26)` | 8..26 | 0.00 (0,0); 0.18 (−amp,0); 0.36 (+amp,0); 0.55 (−amp//2,0); 0.74 (+amp//2,0); 1.00 (0,0) | 360 ms |
| `wobble` | `amp = clamp(int(4 + I*0.12), 5, 16)` | 5..16 | 0.00 (0,0); 0.25 (+amp,+2); 0.50 (−amp,−1); 0.75 (+amp//2,0); 1.00 (0,0) | 420 ms |
| `settle` | `drop = clamp(int(2 + I*0.08), 3, 10)` | 3..10 | 0.00 (0,0); 0.45 (0,+drop); 1.00 (0,0) | 340 ms |

`distance = max(8, min(42, int(10 + intensity * 0.32)))` (used only by forward/back).
All integer arithmetic is C-style truncation toward zero (`int(...)` on a positive float) and `//` floor division.
`forward`/`back` are skipped entirely when `drag_locked` is set.

Cursor direction:

```python
    def _emotion_cursor_direction(self) -> tuple[float, float]:
        center = self._surface_geometry().center()
        cursor = self._current_pointer_position()
        if cursor is None:
            return 0.0, -1.0
        dx = float(cursor.x() - center.x()); dy = float(cursor.y() - center.y())
        length = (dx * dx + dy * dy) ** 0.5
        if length < 8.0:
            return 0.0, -1.0
        return dx / length, dy / length
```

Every keyframe point passes through `_constrained_emotion_point` = `_constrain_position_to_screen(x, y, screen_at(window center))`, i.e. clamped into the screen's available area before being applied.

### 5.4 Expression/motion hold durations for emotion events

```python
            if self._safe_set_expression(model, expression):
                hold_ms = 2600 + int(intensity * 38)          # expression revert
                QTimer.singleShot(hold_ms, ...)
            ...
                    hold_ms = 2600 + int(intensity * 54)      # motion clear
                    QTimer.singleShot(hold_ms, ...)
                    QTimer.singleShot(1800, lambda t=token: self._restore_default_if_finished(t))
```

Motion tag order: tags already present in `source_actions` are moved **to the back**:

```python
        ordered_tags = [tag for tag in tags if tag and tag not in source_actions] + \
                       [tag for tag in tags if tag and tag in source_actions]
```

First tag that resolves to a real motion wins (`MotionPriority.FORCE`, `loop=False`).

---

## 6. Radial menu + tray menu

### 6.1 The 4 items (payload built by `PetWindow._radial_menu_payload`)

```python
        pixel_label = _tr("PetWindow.radial_live2d") if self._pixel_mode else _tr("PetWindow.radial_pixel")
        pixel_glyph = "2D" if self._pixel_mode else "\U0001F47E"        # 🎮 when not in pixel mode
        pixel_color = [66, 142, 214] if self._pixel_mode else [124, 92, 210]
        pixel_enabled = True if self._pixel_mode else bool(pixel_path_for_character(self._current_char))
        return {
            "x": int(gx), "y": int(gy),
            "locked": bool(self._live2d_widget._drag_locked),
            "items": [
                {"action": "chat",    "label": _tr("PetWindow.radial_chat"),    "glyph": "\U0001F4AC", "color": [138, 43, 226],  "enabled": True},
                {"action": "costume", "label": _tr("PetWindow.radial_costume"), "glyph": "\U0001F457", "color": [220, 50, 120],  "enabled": True},
                {"action": "motion",  "label": _tr("PetWindow.radial_motion"),  "glyph": "\U0001F3AC", "color": [30, 144, 255],  "enabled": True},
                {"action": "pixel",   "label": pixel_label,                     "glyph": pixel_glyph,  "color": pixel_color,     "enabled": pixel_enabled},
            ],
        }
```

| # | action id | glyph | color RGB | enabled | i18n label key |
|---|---|---|---|---|---|
| 0 | `chat` | 💬 U+1F4AC | (138, 43, 226) | always | `PetWindow.radial_chat` |
| 1 | `costume` | 👗 U+1F457 | (220, 50, 120) | always | `PetWindow.radial_costume` |
| 2 | `motion` | 🎬 U+1F3AC | (30, 144, 255) | always | `PetWindow.radial_motion` |
| 3 | `pixel` | `2D` (pixel mode) / 🎮 U+1F47E | (66,142,214) pixel mode / (124,92,210) live2d | pixel: True; live2d: only if a pixel sprite exists for the char | `PetWindow.radial_live2d` / `PetWindow.radial_pixel` |

Trigger: **right-click on the model** (`_on_right_click`, after `_is_model_hit_at`) — always sends `SHOW` (never
toggles). Left-click while visible sends `CLOSE` and returns. Plugin items may be appended by
`radial_menu_process.with_plugin_items` (`plugin:<plugin_id>:<id>`, default color `[91, 91, 116]`).

### 6.2 Geometry, sizes, colors (`radial_menu.py`)

```python
        self._radius = 110                                    # in RadialMenu.__init__
        ...
        size = 80                                             # RadialMenuItem.__init__
        self.setFixedSize(size, size)
        ...
        total_w = self._radius * 2 + 80 * 2                   # = 380
        total_h = self._radius * 2 + 80 * 2                   # = 380
        ...
        for i, item in enumerate(self._items):
            angle = -math.pi / 2 + (2 * math.pi * i / n)      # first item at 12 o'clock, clockwise
            dx = int(self._radius * math.cos(angle))
            dy = int(self._radius * math.sin(angle))
            item.end_offset = QPoint(dx, dy)
            item.start_offset = QPoint(0, 0)
            item.widget.move(menu_center.x() - item.widget.width() // 2,
                             menu_center.y() - item.widget.height() // 2)
```

Item paint constants (verbatim numbers):

```python
        r = min(w, h) / 2 - 4                                  # 36 for an 80px item
        color = self._color.lighter(130) if self._hover and self._enabled else self._color
        if not self._enabled:
            color = QColor(120, 120, 120)
        p.setBrush(QColor(0, 0, 0, 34 if self._enabled else 18))          # drop shadow
        p.drawEllipse(QPoint(int(cx), int(cy + 2)), int(r), int(r))
        gradient = QRadialGradient(cx, cy - r * 0.3, r * 1.2)
        gradient.setColorAt(0, color.lighter(162)); gradient.setColorAt(0.62, color); gradient.setColorAt(1, color.darker(116))
        p.setPen(QPen(QColor(255, 255, 255, 92 if self._enabled else 42), 1.4))   # rim
        p.setBrush(QColor(255, 255, 255, 46 if self._hover and self._enabled else 22))  # gloss
        p.drawEllipse(QPoint(int(cx), int(cy - r * 0.22)), int(r * 0.64), int(r * 0.42))
        icon_size = int(r * 0.7)      # drawn at (cx - icon_size/2, cy - icon_size/2 - r*0.15)
        # glyph text: 21 pt, QColor(255,255,255, 240 if enabled else 170)
        # text badge (<=5 ASCII alnum/.+-): 12 pt (<=3 chars) else 10 pt, bold, badge_h = 24,
        #   badge_w = min(r*1.22, max(r*0.86, text_w + 18)), rounded rect radius 9,9,
        #   fill white alpha 46/24, border white alpha 128/64, text white alpha 242/170
        # label: 8 pt bold, QColor(255,255,255, 232/170), elided to int(r * 1.42), drawn at cy + r*0.56
```

Center hub paint:

```python
        rr = 30 * self._center_scale
        base = QColor("#3a3a3a") if self._center_hover else QColor("#2a2a2a")
        p.setOpacity(self._center_opacity)
        p.setPen(QPen(QColor("#555555"), 2))
        gradient = QRadialGradient(cx, cy - rr * 0.2, rr * 1.2)
        gradient.setColorAt(0, base.lighter(140)); gradient.setColorAt(0.7, base); gradient.setColorAt(1, base.darker(140))
        p.drawEllipse(QPoint(int(cx), int(cy)), rr, rr)
        glyph = "\U0001F512" if self._locked else "\U0001F513"     # 🔒 / 🔓
        font.setPointSize(18); p.setPen(QColor(255, 255, 255, 200))
        p.drawText(int(cx - g_w / 2), int(cy + 6), glyph)
```

### 6.3 Drag-lock trigger — **click within 40 px, not a long press**

```python
    def mousePressEvent(self, event: QMouseEvent):
        if event.button() != Qt.MouseButton.LeftButton:
            super().mousePressEvent(event); return
        if any(item.widget.geometry().contains(event.pos()) for item in self._items):
            super().mousePressEvent(event); return
        center = self._center_point(); cx = center.x(); cy = center.y()
        dx = event.pos().x() - cx; dy = event.pos().y() - cy
        dist = (dx * dx + dy * dy) ** 0.5
        if dist < 40:
            self._toggle_locked()
        else:
            self.dismiss()
```

There is **no hold timer anywhere in `radial_menu.py`** — the lock toggles on a single left-press inside the
40 px center disc (`_is_interactive_point` and `mouseMoveEvent` use the same `dist < 40` / `< 40*40` test).
Toggle animation:

```python
        anim.setDuration(180); anim.setStartValue(0.0); anim.setEndValue(1.0)
        anim.setEasingCurve(QEasingCurve.Type.InOutCubic)
        # flips self._locked and emits lock_toggled at value >= 0.5
    def _set_center_anim_value(self, value):
        if value < 0.5:
            t = value / 0.5
            self._center_opacity = 1.0 - t
            self._center_scale = 1.0 - 0.16 * t
        else:
            t = (value - 0.5) / 0.5
            self._center_opacity = t
            self._center_scale = 0.84 + 0.16 * t
    def _set_center_reveal_value(self, value):
        self._center_anim_value = value
        self._center_opacity = value
        self._center_scale = 0.72 + 0.28 * value
```

Show / hide animation timings:

```python
    def _show_duration(self): return 180      # wall-clock ms
    def _hide_duration(self): return 120
    # show: item pos 180ms OutBack; item opacity max(120, 180-50)=130ms;
    #       center reveal max(140, 180-30)=150ms OutBack
    # hide: item pos 120ms InBack; opacity max(80, 120-50)=80ms;
    #       center max(90, 120-20)=100ms InBack
    self._outside_click_timer.setInterval(25)          # poll for outside-click dismiss
```

Menu window: `FramelessWindowHint | Tool | WindowStaysOnTopHint | NoDropShadowWindowHint`,
`WA_TranslucentBackground/WA_NoSystemBackground/WA_ShowWithoutActivating`, `WM_NCCALCSIZE` swallowed on Windows,
`apply_windows_11_border_fix` + `frame_changed` on show, `SetWindowPos(HWND_TOPMOST, …NOACTIVATE)` re-raised at
`(0, 50, 180)` ms. Geometry clamps into `screen.availableGeometry()`.

### 6.4 Tray menu + opacity submenu (`PetWindow._init_tray`)

Order (QSystemTrayIcon path; macOS native status item mirrors it):

1. `PetWindow.tray_show_hide` → `_toggle_visible`
2. `PetWindow.tray_chat` → `_open_chat`
3. `PetWindow.tray_settings` → `_open_settings`
4. separator
5. submenu `PetWindow.tray_opacity` with **exactly** `[100, 80, 60, 40, 20]` percent items, each label
   `PetWindow.opacity_pct` (pct=…) → `self.set_opacity(pct / 100.0)`
6. separator
7. `PetWindow.tray_exit` → `_quit`

```python
        opacity_menu = menu.addMenu(_tr("PetWindow.tray_opacity"))
        for pct in [100, 80, 60, 40, 20]:
            act = opacity_menu.addAction(_tr("PetWindow.opacity_pct", pct=pct))
            act.triggered.connect(lambda checked, v=pct: self.set_opacity(v / 100.0))
            actions.append(act)
```

Tray tooltip keys: `PetWindow.tray_tooltip`, and after a model change
`PetWindow.tray_tooltip_with_model` (display=…, costume=…).

---

## 7. Config defaults + JSON schemas

### 7.1 `config_manager.DEFAULTS` — the subset you asked for (verbatim values)

```python
    "model_picker_state": {
        "recent_characters": [], "favorite_characters": [],
        "recent_costumes": [], "favorite_costumes": [],
    },
    "model_action_settings": {},
    "fps": 120,
    "opacity": 1.0,
    "vsync": True,
    "gpu_acceleration": True,
    "game_topmost": False,
    "obs_window_capture_compatible": False,
    "hide_live2d_model": False,
    "live2d_idle_actions_enabled": True,
    "live2d_random_actions_enabled": True,
    "live2d_quality": "balanced",
    "live2d_scale": 0,
    "moc3_leg_crop_enabled": False,
    "moc3_visible_height": 500,
    "live2d_hit_alpha_threshold": 8,
    "live2d_lip_sync_max_open": 0.55,
    "live2d_head_tracking_enabled": True,
    "live2d_mutual_gaze_enabled": False,
    "emotion_behavior_enabled": True,
    "poke_motion": "",
    "poke_expression": "",
    "move_all_roles_together": False,
    "drag_locked": False,
    "pet_mode": "live2d",
    "window_x": -1, "window_y": -1, "window_width": 400, "window_height": 500,
    "click_motion_profiles": [],
    "click_motion_active_profile": "",
```

Related non-obvious defaults for completeness: `"live2d_idle_actions_enabled": True`,
`"emotion_behavior_enabled": True`, `"pet_mode": "live2d"`, `"window_placement": {}`.

`live2d_scale = 0` means "auto from device pixel ratio": `clamp_live2d_scale(value, default=100, use_device_pixel_ratio_default=True)`
→ `pct = int(round(primaryScreen.devicePixelRatio()*100))` when `pct <= 0`; then clamped to **25..500**
(`LIVE2D_SCALE_MIN=25`, `LIVE2D_SCALE_MAX=500`).

`live2d_quality` profiles (`live2d_quality.py`), unknown values normalize to `"balanced"`:

```python
LIVE2D_QUALITY_PROFILES = {
    "performance": {"texture_scale": 0.5, "use_mipmap": False, "bleed_passes": 0},
    "balanced":    {"texture_scale": 1.0, "use_mipmap": True,  "bleed_passes": 1},
}
LIVE2D_MOC3_BASE_HEIGHT = 800; LIVE2D_MOC3_VISIBLE_HEIGHT_MIN = 500
LIVE2D_MOC3_RENDER_SCALE = 1.45
```

### 7.2 `click_motion_actions` schema (stored per model entry, canonical / normalized form)

Only regions present in `CLICK_MOTION_REGIONS` are kept; entries with both fields empty are dropped; invalid
motion/expression names (when a valid-name set is supplied) are blanked; `""`/`"__random__"`/`"__none__"` always survive
as motion values.

```json
{
  "head":                {"motion": "shame01", "expression": "f01"},
  "upper_body_left":     {"motion": "",        "expression": "f01"},
  "upper_body_center":   {"motion": "angry01", "expression": "f01"},
  "upper_body_right":    {"motion": "",        "expression": "f01"},
  "lower_body_left":     {"motion": "shame01", "expression": "f01"},
  "lower_body_center":   {"motion": "angry01", "expression": "f01"},
  "lower_body_right":    {"motion": "serious01","expression": "f01"}
}
```

(That example is literally the `tsundere` preset resolved against kasumi/live_default, one motion chosen per region.)

Extreme cases:
* `auto` preset ⇒ `{"head": {"motion": "", "expression": ""}, …}` for all 7 regions (all values empty ⇒ the
  normalizer would drop every key, so an all-auto model entry ends up as `"click_motion_actions": {}`).
* `random` preset ⇒ `{"head": {"motion": "__random__", "expression": ""}, …}` for all 7.
* A bare string is accepted on input: `"head": "smile01"` ⇒ `{"motion": "smile01", "expression": ""}`.

Interpreter (`normalize_click_motion_actions`, verbatim body):

```python
    for region, raw_value in actions.items():
        region = str(region)
        if region not in valid_regions:
            continue
        if isinstance(raw_value, dict):
            motion = str(raw_value.get("motion", "") or "").strip()
            expression = str(raw_value.get("expression", "") or "").strip()
        else:
            motion = str(raw_value or "").strip()
            expression = ""
        if not motion and not expression:
            continue
        if valid_motion_set is not None and motion not in CLICK_MOTION_SPECIAL_VALUES and motion not in valid_motion_set:
            motion = ""
        if valid_expression_set is not None and expression not in valid_expression_set:
            expression = ""
        if not motion and not expression:
            continue
        normalized[region] = {"motion": motion, "expression": expression}
```

### 7.3 `click_motion_profiles` schema (user-saved reusable profiles)

List; built-in names (`auto, genki, tsundere, shy, cool, surprised, random`) are **rejected** on save and filtered out on load.
`normalize_click_motion_profile` keeps only `{name, click_motion_actions}` (legacy format stored actions directly
under the profile root, hence the `profile.get("click_motion_actions", {})` read).

```json
[
  {
    "name": "my_profile",
    "click_motion_actions": {
      "head":              {"motion": "smile01", "expression": "f01"},
      "upper_body_center": {"motion": "kime01",  "expression": ""}
    }
  }
]
```

```python
def normalize_click_motion_profile(profile) -> dict | None:
    if not isinstance(profile, dict): return None
    name = str(profile.get("name", "")).strip()
    if not name: return None
    actions = {}
    raw_actions = profile.get("click_motion_actions", {})
    if isinstance(raw_actions, dict):
        for region in CLICK_MOTION_REGIONS:
            if region in raw_actions and isinstance(raw_actions[region], dict):
                motion = str(raw_actions[region].get("motion", "")).strip()
                expression = str(raw_actions[region].get("expression", "")).strip()
                if motion or expression:
                    actions[region] = {"motion": motion, "expression": expression}
    return {"name": name, "click_motion_actions": actions}
```

`click_motion_active_profile` is a plain string (the currently selected profile name, `""` = 当前自定义).
Dedup/order: `_normalize_click_motion_profiles` keeps first occurrence of each name, drops built-ins, and clears
`click_motion_active_profile` if it is neither a kept custom name nor a built-in name.

### 7.4 `model_action_settings` schema (keyed by `character\tcostume`)

```python
def model_action_settings_key(character: str, costume: str) -> str:
    return f"{character}\t{costume}"     # real TAB character

MODEL_ACTION_KEYS = ("default_motion", "default_expression", "click_motion_profile_name", "click_motion_actions")
```

Normalized entry keeps only non-empty values:

```json
{
  "kasumi\tlive_default": {
    "default_motion": "idle02",
    "default_expression": "f01",
    "click_motion_profile_name": "tsundere",
    "click_motion_actions": {
      "head":              {"motion": "shame01", "expression": "f01"},
      "upper_body_center": {"motion": "angry01", "expression": "f01"}
    }
  }
}
```

```python
def normalize_model_action_profile(profile) -> dict:
    if not isinstance(profile, dict): return {}
    normalized = {}
    default_motion = str(profile.get("default_motion", "")).strip()
    default_expression = str(profile.get("default_expression", "")).strip()
    click_motion_profile_name = str(profile.get("click_motion_profile_name", "")).strip()
    click_motion_actions = normalize_click_motion_actions(profile.get("click_motion_actions", {}))
    if default_motion: normalized["default_motion"] = default_motion
    if default_expression: normalized["default_expression"] = default_expression
    if click_motion_profile_name: normalized["click_motion_profile_name"] = click_motion_profile_name
    if click_motion_actions: normalized["click_motion_actions"] = click_motion_actions
    return normalized
```

`set_model_action_profile` deletes the key entirely when the normalized profile is empty; empty character or
costume ⇒ no-op. `PetWindow._with_saved_action_profile` merges the profile into the `models[]` entry, and
**the entry wins** — profile values only fill keys that are falsy in the entry:

```python
        for key in ("default_motion", "default_expression", "click_motion_actions"):
            if not merged.get(key) and profile.get(key):
                merged[key] = profile[key]
```

### 7.5 `models[]` entry schema (`MODEL_DEFAULTS` + character/costume)

```python
MODEL_DEFAULTS = {
    "window_x": -1, "window_y": -1, "window_width": 400, "window_height": 500,
    "window_placement": {},
    "pixel_window_x": -1, "pixel_window_y": -1, "pixel_window_placement": {},
    "pet_mode": "live2d",
    "default_motion": "",
    "default_expression": "",
    "click_motion_profile_name": "",
    "click_motion_actions": {},
}
```

```json
{
  "character": "kasumi",
  "costume": "live_default",
  "window_x": 1200, "window_y": 300, "window_width": 400, "window_height": 500,
  "window_placement": {},
  "pixel_window_x": -1, "pixel_window_y": -1, "pixel_window_placement": {},
  "pet_mode": "live2d",
  "default_motion": "",
  "default_expression": "",
  "click_motion_profile_name": "",
  "click_motion_actions": {}
}
```

`_current_model_entry()` matches on BOTH `character` and `costume`; if no exact costume match it falls back to the
**first entry of that character**; `{}` if nothing matches.

### 7.6 Related behaviors for those booleans

* `drag_locked` — `live2d_widget._drag_locked`; in `mousePressEvent` it accepts the press but never starts a drag;
  `forward`/`back` emotion motions are suppressed while locked; radial payload reports `"locked"`.
* `game_topmost` — Windows `SetWindowPos(HWND_TOPMOST, …, SWP_NOMOVE|SWP_NOSIZE|SWP_NOACTIVATE)`; Wayland
  `StackMode.GAME_OVERLAY` instead of `StackMode.TOP`; macOS `set_window_level_above_menu_bar` vs `set_window_level_status_bar`.
* `obs_window_capture_compatible` — ex-style swap: ON = `(style & ~WS_EX_TOOLWINDOW) | WS_EX_APPWINDOW`,
  OFF = `(style & ~WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW`, then `SWP_FRAMECHANGED`.
* `move_all_roles_together` — when true, a drag on one pet broadcasts a per-tick delta to peers
  (`_broadcast_peer_drag(total_dx, total_dy)`, throttled by `PEER_POS_BROADCAST_INTERVAL_MS = 200`).

---

## 8. `band.json` / `outfit.json` schemas + costume name lookup

### 8.1 `band.json` — one top-level key: `bands` (array, 13 entries)

```json
{
  "bands": [
    {
      "id": "poppin_party",
      "display": "Poppin'Party",
      "logo": "band_logo/poppinparty.png",
      "characters": ["kasumi", "tae", "rimi", "saaya", "arisa"]
    }
  ]
}
```

Fields: `id` (string), `display` (string), `logo` (string, optional — the last entry `others` has none),
`characters` (array of character keys). Actual band ids in order:
`poppin_party, afterglow, pastel_palettes, hello_happy_world, roselia, raise_a_suilen, morfonica, mygo, ave_mujica, yumemita, millsage, ikka_dumb_rock, others`
(`others` → `["mana","asuka","kanata","viola"]`, display `其他角色`).

Parsing (`ModelManager._parse_band_json`): bands keep only characters that exist in `_characters` **and** have
costumes; characters not listed in `band.json` are appended as one extra band
`{"id": …, "display": …?, "characters": ungrouped}` (ungrouped = present in `characters` but in no configured band).

### 8.2 `outfit.json` — one top-level key: `characters` (object, 64 entries)

```json
{
  "characters": {
    "kasumi": {
      "display": "户山香澄",
      "costumes": {
        "2018_dog": "狗狗",
        "casual": "常服",
        "live_default": "队服",
        "casual_summer-2023": "夏季常服2023",
        "collabo_d_1_ur": "collabo_d_1_ur"
      }
    }
  }
}
```

Per character: exactly two properties — `display` (Chinese full name) and `costumes` (map `costume_id → 中文服装名`).
Real sample entry sizes: `kasumi` has **120** costume keys in `outfit.json` (116 of which exist in the archive).
Costume ids match the on-disk directory name / archive member parent directory 1:1.
Some display values equal the id itself (`"collabo_d_1_ur": "collabo_d_1_ur"`) — do not assume Chinese text.

### 8.3 Exact lookup rules

```python
    def _parse_outfit_json(self):
        chars = data.get("characters", {})
        for key, info in chars.items():
            self._characters.setdefault(key, {})
            self._characters[key]["display"] = info.get("display", key)
            costumes = info.get("costumes", {})
            if costumes:
                self._costume_names.setdefault(key, {})
                self._costume_names[key].update(costumes)

    def get_display_name(self, character: str) -> str:
        return self._characters.get(character, {}).get("display", character.title())

    def get_costume_display_name(self, character: str, costume_id: str) -> str:
        return self._costume_names.get(character, {}).get(costume_id, costume_id)
```

* **Key format**: `characters[<character_key>].costumes[<costume_id>]` where `<costume_id>` is the exact directory /
  archive-member-parent name (case-sensitive, underscores and hyphens preserved: `casual_summer-2023`).
* Lookup: exact dict hit; miss ⇒ the **raw costume id is displayed** (never empty, never localized differently).
* Character display name miss ⇒ Python `str.title()` of the key (i.e. `"kasumi" → "Kasumi"`, `"chu2" → "Chu2"`).

### 8.4 Index scanning / virtual paths / picker state

Search roots (`model_manager.py`):

```python
BASE_DIR = app_base_dir(); DATA_DIR = app_data_dir()
MODELS_DIR = DATA_DIR / "models"
BUNDLED_MODELS_DIR = BASE_DIR / "models"
OUTFIT_JSON = BASE_DIR / "outfit.json"; BAND_JSON = BASE_DIR / "band.json"
CHARACTERS_DIR = BASE_DIR / "characters"
MODEL_FORMAT_MOC = "moc"; MODEL_FORMAT_MOC3 = "moc3"
ARCHIVE_SCAN_CACHE_NAME = ".bandori_model_index.json"; ARCHIVE_SCAN_CACHE_VERSION = 1
_MODEL_JSON_CACHE_LIMIT = 32

def model_search_dirs():
    candidates = (BUNDLED_MODELS_DIR, MODELS_DIR) if getattr(sys, "frozen", False) else (MODELS_DIR,)
def model_lookup_dirs():
    candidates = (MODELS_DIR, BUNDLED_MODELS_DIR) if getattr(sys, "frozen", False) else (MODELS_DIR,)
```

* Entries starting with `_` are skipped; `override = root == MODELS_DIR and root != BUNDLED_MODELS_DIR` (user data dir wins over bundled).
* Directory model source: `models/<character>/<costume>/` — manifest candidates are `model.json` first, then
  `*.model3.json` (sorted). Validity requires `Moc`(+`Textures` list) resolvable on disk (`FileReferences` for moc3,
  top-level `model`/`textures` for moc2); absolute resource paths are rejected.
* Archive model source: `models/<character>.zst` (tar.zst). `character = archive_path.stem`; for every member that is
  a model manifest: `parent = member.rsplit("/", 1)[0] if "/" in member else ""`,
  `costume_id = parent.rsplit("/", 1)[-1] if parent else "default"`, and
  `model_path = f"{archive_resolved}::{"{member}"}"`, `format = moc3 if member.lower().endswith(".model3.json") else moc`.
  Costumes are sorted by `id`; cached in `.bandori_model_index.json` (`version`, `archives[<resolved path>] = {size, mtime_ns, result}`).
* Virtual path separator: `VIRTUAL_SEP = "::"` (`zst_model_archive.py`):

```python
def is_virtual_path(path: str) -> bool: return VIRTUAL_SEP in str(path)
def make_virtual_path(archive_path, member_path) -> str:
    return f"{Path(archive_path).resolve()}{VIRTUAL_SEP}{_normalize_member(member_path)}"
def split_virtual_path(path: str) -> tuple[str, str]:
    archive_path, member_path = str(path).split(VIRTUAL_SEP, 1)
```

  Real observed value: `C:\oss\BANDORI-PET-REV\models\kasumi.zst::live_default/model.json`.
  Any code doing `path.split("::", 1)[0]` gets the archive file (see `outfit_description.py`);
  `Path(path)` on a virtual path is meaningless — always branch on `is_virtual_path` first.
  Note the `::` collides with IPv6 literals only outside paths (`asr_manager` treats `"::"`/`"::1"` as host names).

* `get_model_json_path` order: `_model_paths[(character, costume)]` first, then filesystem fallback
  `model_lookup_dirs()` → `root/character/costume` manifest (resolved absolute). `""` if none.
* `get_model_format`: costume metadata first (`moc`/`moc3`), else inferred from path (`_model_format_from_path`).
* `get_motion_names` / `get_expression_names`: read the model json (LRU 32), prefer top-level `motions`/`expressions`
  (moc2), fall back to `FileReferences.Motions` / `FileReferences.Expressions` (moc3); names extracted from each
  expression dict's `name` or `Name`; **both lists returned `sorted()`**; `[]` on any error.
* Default costume preference:

```python
        preferred = ["live_default", "casual", "school_winter", "school_summer"]
        # first preferred id present in costume_ids, else costumes[0]["id"]
```

* Picker state (`settings_window/constants.py`, `settings_window.py`):

```python
MODEL_PICKER_STATE_KEY = "model_picker_state"
MODEL_PICKER_RECENT_LIMIT = 8
MODEL_PICKER_FILTER_ALL = "all"; MODEL_PICKER_FILTER_RECENT = "recent"; MODEL_PICKER_FILTER_FAVORITES = "favorites"

    @staticmethod
    def _costume_key(character: str, costume: str) -> str: return f"{character}:{costume}"   # NOTE: single colon
    @staticmethod
    def _split_costume_key(key: str) -> tuple[str, str]:
        if ":" not in key: return "", ""
        character, costume = key.split(":", 1)
        return character, costume
```

  * `recent_characters` / `favorite_characters`: character keys, deduped, `recent` capped at **8**
    (`state["recent_characters"] = state["recent_characters"][:MODEL_PICKER_RECENT_LIMIT]`).
  * `recent_costumes` / `favorite_costumes`: `"character:costume"` keys (single colon!), `recent_costumes` capped at 8.
  * Recent insert: remove existing then `insert(0, key)`; favorites: same order rule (`insert(0, …)`).
  * Validity filter: characters must exist in `ModelManager.characters`; costume keys must satisfy
    `model_manager.get_model_json_path(character, costume)` non-empty.
  * Persisted state is always written whole (all four lists).

---

## 9. Constant inventory (all values verified in source)

### Head tracking (`live2d_widget_base.py`)
```python
        self._head_track_min_delta_sq = 16        # deadzone: skip if dx²+dy² < 16  → 4 px, not 2
        max_dist = 600.0                          # head-tracking radius in px
        factor = max(0.0, min(1.0, (now - self._neutral_head_started_at) / 0.3))   # 0.3 s recenter
```
* Tracking point = window center (`_cache_global_x + _cache_w_half`, `_cache_global_y + _cache_h_half`); if the cursor is
  farther than 600 px, the point is projected onto the 600 px circle:
  `factor = 600.0 / dist; local_x = half_w + dx*factor; local_y = half_h + dy*factor`, then `model.Drag(local_x, local_y)`.
* Recenter: linear lerp from the last head target to the window center over **0.3 s**, then a final snap when `factor >= 1.0`.
* Skipped while dragging or with no model; the cache update can force a sample (`widget_moved` bypasses the deadzone).
* Wayland: external pointer sample accepted if `age_us <= 250_000` (250 ms); local fallback if pointer sample ≤ `0.25 s` old.
* Gaze target (`set_gaze_target`) overrides head tracking, and mutual gaze takes priority over head tracking
  (enabling mutual gaze disables head tracking in settings).
* Deadzone caveat: `2 px` appears nowhere; the real value is `sqrt(16) = 4 px`. If you want 2 px you'd use sq=4.

### Mutual gaze (`pet_window.py`)
```python
PEER_POS_BROADCAST_INTERVAL_MS = 200      # peer window-center broadcast cadence
```
* Target = the **nearest peer window center** by squared distance from own center (`_peer_window_positions.values()`),
  then `live2d_widget.set_gaze_target(tx, ty)`; cleared when the feature is off or no peers.
* Recomputed on own `moveEvent`, on peer position updates, and from the main tick when enabled.

### Approach behavior (ambient "notice the cursor")
```python
LIVE2D_MOUSE_APPROACH_COOLDOWN_SECONDS = 150
LIVE2D_MOUSE_APPROACH_MIN_IDLE_SECONDS = 12
LIVE2D_MOUSE_APPROACH_DWELL_SECONDS = 4.0
LIVE2D_MOUSE_APPROACH_RADIUS = 180
LIVE2D_MOUSE_APPROACH_EXIT_RADIUS = 270
```
```python
        approach_radius = max(LIVE2D_MOUSE_APPROACH_RADIUS,
                              min(360, int(max(self.width(), self.height()) * 0.42)))
        exit_radius = max(LIVE2D_MOUSE_APPROACH_EXIT_RADIUS, approach_radius + 80)
```
Gate order: inside exit rect → inside `exit_radius` → `near = dist_sq <= approach_radius²` →
latch `_cursor_near_live2d_since` on entry → require `now - since >= 4.0 s` → require user idle ≥ 12 s →
require `now - _last_mouse_approach_action_at >= 150 s` → run `"approach"` behavior.
Weighted pick for approach: motions `smile:3, nf:3, idle02:2, surprised:1`; expressions `smile:4, default:3, idle:2, surprised:1`
(weights implemented by repeating the resolved name in a list, then `random.choice`).

### Random/idle motion tick + time-of-day buckets
```python
LIVE2D_CONTEXT_IDLE_INTERVAL_MS = 5000     # 5 s tick (QTimer)
LIVE2D_DAZE_AFTER_SECONDS  = 7 * 60        # 7 min idle  → "daze"
LIVE2D_SLEEP_AFTER_SECONDS = 18 * 60       # 18 min idle → "sleep"
LIVE2D_AMBIENT_COOLDOWN_SECONDS = 180      # min gap between ambient actions (also 180 s)
LIVE2D_BASE_WIDTH = 400; LIVE2D_BASE_HEIGHT = 500
```
```python
    def _context_idle_kind(self, idle_seconds: float) -> str:
        hour = time.localtime().tm_hour
        today = time.strftime("%Y-%m-%d")
        if 5 <= hour < 10 and f"{today}:morning" not in self._daily_context_idle_seen:
            return "morning"
        if idle_seconds >= LIVE2D_SLEEP_AFTER_SECONDS:
            return "sleep"
        if (hour >= 23 or hour < 5) and idle_seconds >= 90:
            return "late_night"
        if idle_seconds >= LIVE2D_DAZE_AFTER_SECONDS:
            return "daze"
        return ""
```
Priority: `morning` (05:00–09:59, once per day) > `sleep` (≥18 min) > `late_night` (23:00–04:59 and ≥90 s idle)
> `daze` (≥7 min). Ambient action requires `now - _last_context_idle_action_at >= 180 s`; `morning` is recorded in
`_daily_context_idle_seen` as `"YYYY-MM-DD:morning"`. Motion/expression tag lists per kind:
`morning` motions `("stretch","akubi","sigh","smile","kime","idle02")`, expressions `("smile","default","idle")`;
`late_night`/`sleep` `("sleep","akubi","sigh","sad","idle02")` / `("sad","sleep","idle","default")`;
`daze` `("stare","mitore","thinking","eeto","odoodo","nf","idle02")` / `("idle","default","sad")`.
After an idle motion starts: clear at **9000 ms**, restore default-if-finished at **1800 ms**; expression revert at **6000 ms**.

### Poke bubble (double-click / remote poke on user)
```python
POKE_USER_BADGE_DURATION_MS = 1980
POKE_USER_WINDOW_SHAKE_INTENSITY = 36
```
Badge: `QLabel`, brand background `BANDORI_PRIMARY = "#e4004f"`, white text, `border: 1px solid rgba(255,255,255,180)`,
`border-radius: 15px`, `padding: 6px 13px`, bold 10 pt, min size `max(96,w) × max(30,h)`.
Motion keyframes (fractions of 1980 ms, `OutCubic`): pos `start = settle+(0,+12)`; `0.22 → settle`; `0.72 → settle`;
`end = settle+(0,-8)`. Opacity: `0.0 → @0.14 1.0 → @0.72 1.0 → 0.0`.
Position: `x = model_center_x - w/2`, `y = model_top - h - 8`, margin 8 px clamped into the window;
fallback when no hit-area bounds: `model_top = top + max(14, int(round(height * 0.20)))`.
Text: event `message` whitespace-collapsed, else `"{display}戳了你一下"`; truncated to **32** chars as `text[:29].rstrip() + "..."`.
Window reaction: `_play_emotion_window_feedback("shake", 36)` for `direction == "to_user"`, and `("shake", 72)` for
local double-click / character-poking-user feedback.

### Stability compensation (hit test + passthrough)
```python
# live2d_widget_base.py
DEFAULT_HIT_ALPHA_THRESHOLD = 8
DEFAULT_LIP_SYNC_MAX_OPEN = 0.55
HIT_STABILITY_GRACE_MS = 120
HIT_STABILITY_DISTANCE = 12
MAX_CONSECUTIVE_DRAW_FAILURES = 3
DRAW_FAILURE_LOG_INTERVAL_SECONDS = 5.0
```
```python
            if state is True:
                self._last_confirmed_hit_ms = now
                self._last_confirmed_hit_pos = (float(x), float(y))
                return True
            last_pos = self._last_confirmed_hit_pos
            if (last_pos is not None
                and now - self._last_confirmed_hit_ms < HIT_STABILITY_GRACE_MS):
                dx = float(x) - last_pos[0]; dy = float(y) - last_pos[1]
                if dx * dx + dy * dy <= HIT_STABILITY_DISTANCE ** 2:
                    return True
            return False
```
```python
# pet_window.py  (mouse passthrough)
MOUSE_PASSTHROUGH_INTERVAL_MS = 16
MOUSE_PASSTHROUGH_IDLE_INTERVAL_MS = 125
MOUSE_PASSTHROUGH_EDGE_MARGIN = 96
MOUSE_PASSTHROUGH_HIT_GRACE_SECONDS = 0.08      # 80 ms
MOUSE_PASSTHROUGH_HIT_GRACE_DISTANCE = 12
```
```python
        if (last_pos is not None
            and now - self._mouse_passthrough_last_hit_at < MOUSE_PASSTHROUGH_HIT_GRACE_SECONDS):
            dx = global_pos.x() - last_pos[0]; dy = global_pos.y() - last_pos[1]
            if dx * dx + dy * dy <= MOUSE_PASSTHROUGH_HIT_GRACE_DISTANCE ** 2:
                return False          # keep the window interactive
        return True                   # passthrough
```
Hit rule: alpha `> _hit_alpha_threshold` (default 8, clamped 0..255) counts as opaque/hit; a recent confirmed hit
within 120 ms (widget) / 80 ms (window passthrough) and within 12 px keeps the verdict "hit" so animation frames
don't flicker the click through.

### Other window constants (`pet_window.py`)
```python
TOPMOST_INTERACTION_REFRESH_SECONDS = 0.25
TOPMOST_GUARD_INTERVAL_MS = 1000
TOPMOST_RECOVERY_DELAYS_MS = (0, 250, 1000, 2500)
LIVE2D_PREWARM_MAX_MOTION_FILES = 24
LIVE2D_PREWARM_MAX_EXPRESSIONS = 12
LIVE2D_PREWARM_STEP_MS = 24
LIVE2D_PREWARM_WAIT_MS = 25
STARTUP_POSITION_RESTORE_RETRY_DELAYS_MS = (0, 100, 300, 800, 1600, 3000)
WM_NCHITTEST = 0x0084; WM_NCCALCSIZE = 0x0083; HTTRANSPARENT = -1
WM_DISPLAYCHANGE = 0x007E; WM_POWERBROADCAST = 0x0218; WM_WTSSESSION_CHANGE = 0x02B1
PBT_APMRESUMECRITICAL = 0x0006; PBT_APMRESUMESUSPEND = 0x0007; PBT_APMRESUMEAUTOMATIC = 0x0012
WTS_SESSION_UNLOCK = 0x0008
GWL_EXSTYLE = -20; WS_EX_TRANSPARENT = 0x00000020; WS_EX_TOOLWINDOW = 0x00000080
WS_EX_APPWINDOW = 0x00040000; WS_EX_NOACTIVATE = 0x08000000
HWND_TOPMOST = -1
```

### DWM attributes (`win32_dwm.py`, complete)
```python
DWMWA_WINDOW_CORNER_PREFERENCE = 33
DWMWA_BORDER_COLOR = 34
DWMWCP_DONOTROUND = 1
DWMWA_COLOR_NONE = 0xFFFFFFFE
SWP_NOSIZE = 0x0001; SWP_NOMOVE = 0x0002; SWP_NOZORDER = 0x0004
SWP_NOACTIVATE = 0x0010; SWP_FRAMECHANGED = 0x0020
```
* `is_windows_11_or_later()` = `dwMajorVersion >= 10 and dwBuildNumber >= 22000` via `ntdll.RtlGetVersion`
  (deliberately not `GetVersionEx`, which lies under a manifest).
* `apply_no_rounding(hwnd, windows_11_only=False)` → `DwmSetWindowAttribute(hwnd, 33, 1, 4)`.
* `apply_no_border(hwnd)` → `DwmSetWindowAttribute(hwnd, 34, 0xFFFFFFFE, 4)`.
* `apply_windows_11_border_fix` = both; `frame_changed(hwnd)` = `SetWindowPos(hwnd, None, 0,0,0,0, NOMOVE|NOSIZE|NOZORDER|NOACTIVATE|FRAMECHANGED)`.
* Value is passed as `ctypes.c_int` + `sizeof` (DWORD 4). All calls swallow exceptions.

---

## 10. Rust port checklist (ordering + gotchas)

1. Tag→name matching must keep the **three-branch `elif` chain** and the `thinking` candidate expansion; the regex is
   `(^|[_\-])TAG($|[_\-]?\d)` on the lower-cased name (Rust: `format!(r"(^|[_\-]){}($|[_\-]?\d)", regex::escape(tag))`).
2. `char_name` empty ⇒ branch 1 and 2 coincide ⇒ names duplicated in the candidate pool (harmless for uniform choice,
   relevant if you ever weight by pool length).
3. Presets are resolved **against a specific model's** motion/expression lists and stored as literal names; the pet
   runtime never re-resolves tags. Store `click_motion_actions` + `click_motion_profile_name` per model.
4. `""` motion means AUTO (region buckets), `"__random__"` means any prewarmed non-idle motion, `"__none__"` means
   do nothing at all (expression is ignored).
5. Expression resolution is deterministic-first-match and ends with `expression_names[0]` — with `f01..f18` packs every
   tag yields `f01`.
6. Window motion keyframes are fractions of the duration, OutCubic-eased per segment, and every point is screen-clamped
   before being applied; `forward`/`back` are positional deltas from the *current* position and are blocked by drag lock.
7. Radial menu: 4 items at 90° steps starting at −90° (12 o'clock, clockwise), radius 110, item box 80, window 380×380,
   center hub radius 30 (interactive radius 40), lock toggles on press (no long press), show 180 ms / hide 120 ms.
8. Region math is only ratios: head cut `y<0.38`, body split `y<0.64`, x splits `0.38 / 0.62` — with hit-area-relative
   normalization whenever the area bbox is larger than 1×1 px.
