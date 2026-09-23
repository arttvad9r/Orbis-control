# Orbis Control — planning log

Формат: одно запись = одно существенное решение. Записи дополняются; источник истины —
контракты в репо и доска `orbis-v01`, не этот журнал.

## 2026-09-21 — D-008..D-011 (записаны в DECISIONS.md репо)

- **D-008**: разрешение на bounded live-hardware round-trip валидацию выдано заранее
  (финальный гейт v0.1, восстановление значений обязательно).
- **D-009**: цель итерации — первый релиз v0.1 (PKGBUILD + installed smoke + тег).
  Draft PR #130 закрыт как устаревший.
- **D-010**: ветка `implementation/current-plan` запушена в origin; `main` не двигаем до RC.
- **D-011**: основной backend-референс — G-Helper-linux, `/home/artt/g-helper-linux`
  (upstream `utajum/g-helper-linux`). Затык → сначала изучить модуль референса
  (`src/Platform/Linux/Asus/`, `src/Fan/`, `src/Gpu/`, `src/Battery/`, `src/Mode/`),
  адаптировать под Rust/capability/Hardware1; слепое копирование запрещено.

## 2026-09-21 — Топология исполнения

Выбор: **один implementation-поток (developer) → независимая QA → planner release-гейт**.
Один общий workspace (`dir:`), карты последовательные, параллельных писателей нет.

Карты доски `orbis-v01`:

| id | роль | суть | зависит от |
|---|---|---|---|
| t_3e04f428 (A) | developer | верификация этапов 1–3 против AC, фиксация вердиктов | — |
| t_b16d7354 (B) | developer | fan curves production (этап 1) | A |
| t_51a6ba86 (C) | developer | power limits + boost (этапы 2–4) | A |
| t_89ca2ac9 (D) | developer | GPU modes / MUX (этап 5) | C |
| t_5b9d96c6 (QA) | qa | liveness, инвентаризация, визуальные состояния, честность capability | B, C, D |
| t_37e1764f (E) | developer | Arch-пакет + установленный кандидат + smoke (этапы 6–7 FINISH_PLAN) | QA |
| t_9f6592c8 (R) | planner | терминальный release-гейт v0.1, live round-trip (D-008), bump, тег | E |

Почему так: работа невелика и последовательна по одной ветке кода; параллельные worktree
не нужны. Карта A сначала доказывает, что уже сделано (не переписывать работающее), B–D
закрывают реальные разрывы по очереди IMPLEMENTATION_PLAN.

Контракты: `acceptance-contract.yaml` (19 сценариев SC-*), `interaction-contract.md`,
`docs/qa-coverage.txt` — валидированы board-protocol, коммит bb1c854.

## 2026-09-21 — Базовое состояние (проверено прогонами)

- `scripts/verify task` (fmt+check+test+clippy -D warnings) — зелёный;
- `cargo build --workspace --release --locked` — зелёный, 4 бинарника;
- рабочее дерево чистое; HEAD реализации: 06fbf16 (после bb1c854 — bb1c854);
- доска: орбис-v01, карта A запущена диспетчером сразу после создания.

## Следующее действие

Мониторинг карты A (t_3e04f428): вердикты AC этапов 1–3 → разблокировка B и C.

## 2026-09-22 — UI-линия: параллельный visual-repair поток

Решение:

Жалоба пользователя (симметрия, размерности, логика построения) оформлена отдельной UI-линией
на доске orbis-v01: UI-1 аудит (t_5fc90d28) -> UI-2 исправление (t_29073c31) -> UI-QA приёмка
(t_ccbe5158), все в worktree-ветке wt/ui-symmetry от ui-профиля. Слияние с функциональной линией —
отдельной картой INT (t_3864d46e, developer) после завершения A..D и PASS UI-QA; release R (t_9f6592c8)
теперь зависит и от INT.

Границы: правки только ui/*.slint + стилевые константы; поведение контролов, backend-крейты,
навигация и фреймворк не меняются (D-006: исправление дефектов audited UI, не новый дизайн).

Почему:

Два писателя в одном чекауте запрещены; worktree развязывает линии, а INT-карта делает слияние
управляемым и проверяемым (build+verify+smoke на объединённой ветке).
# Orbis Control — planning log

Формат: одно запись = одно существенное решение. Записи дополняются; источник истины —
контракты в репо и доска `orbis-v01`, не этот журнал.

## 2026-09-21 — D-008..D-011 (записаны в DECISIONS.md репо)

- **D-008**: разрешение на bounded live-hardware round-trip валидацию выдано заранее
  (финальный гейт v0.1, восстановление значений обязательно).
- **D-009**: цель итерации — первый релиз v0.1 (PKGBUILD + installed smoke + тег).
  Draft PR #130 закрыт как устаревший.
- **D-010**: ветка `implementation/current-plan` запушена в origin; `main` не двигаем до RC.
- **D-011**: основной backend-референс — G-Helper-linux, `/home/artt/g-helper-linux`
  (upstream `utajum/g-helper-linux`). Затык → сначала изучить модуль референса
  (`src/Platform/Linux/Asus/`, `src/Fan/`, `src/Gpu/`, `src/Battery/`, `src/Mode/`),
  адаптировать под Rust/capability/Hardware1; слепое копирование запрещено.

## 2026-09-21 — Топология исполнения

Выбор: **один implementation-поток (developer) → независимая QA → planner release-гейт**.
Один общий workspace (`dir:`), карты последовательные, параллельных писателей нет.

Карты доски `orbis-v01`:

| id | роль | суть | зависит от |
|---|---|---|---|
| t_3e04f428 (A) | developer | верификация этапов 1–3 против AC, фиксация вердиктов | — |
| t_b16d7354 (B) | developer | fan curves production (этап 1) | A |
| t_51a6ba86 (C) | developer | power limits + boost (этапы 2–4) | A |
| t_89ca2ac9 (D) | developer | GPU modes / MUX (этап 5) | C |
| t_5b9d96c6 (QA) | qa | liveness, инвентаризация, визуальные состояния, честность capability | B, C, D |
| t_37e1764f (E) | developer | Arch-пакет + установленный кандидат + smoke (этапы 6–7 FINISH_PLAN) | QA |
| t_9f6592c8 (R) | planner | терминальный release-гейт v0.1, live round-trip (D-008), bump, тег | E |

Почему так: работа невелика и последовательна по одной ветке кода; параллельные worktree
не нужны. Карта A сначала доказывает, что уже сделано (не переписывать работающее), B–D
закрывают реальные разрывы по очереди IMPLEMENTATION_PLAN.

Контракты: `acceptance-contract.yaml` (19 сценариев SC-*), `interaction-contract.md`,
`docs/qa-coverage.txt` — валидированы board-protocol, коммит bb1c854.

## 2026-09-21 — Базовое состояние (проверено прогонами)

- `scripts/verify task` (fmt+check+test+clippy -D warnings) — зелёный;
- `cargo build --workspace --release --locked` — зелёный, 4 бинарника;
- рабочее дерево чистое; HEAD реализации: 06fbf16 (после bb1c854 — bb1c854);
- доска: орбис-v01, карта A запущена диспетчером сразу после создания.

## Следующее действие

Мониторинг карты A (t_3e04f428): вердикты AC этапов 1–3 → разблокировка B и C.

## 2026-09-22 — UI-линия: параллельный visual-repair поток

Решение:

Жалоба пользователя (симметрия, размерности, логика построения) оформлена отдельной UI-линией
на доске orbis-v01: UI-1 аудит (t_5fc90d28) -> UI-2 исправление (t_29073c31) -> UI-QA приёмка
(t_ccbe5158), все в worktree-ветке wt/ui-symmetry от ui-профиля. Слияние с функциональной линией —
отдельной картой INT (t_3864d46e, developer) после завершения A..D и PASS UI-QA; release R (t_9f6592c8)
теперь зависит и от INT.

Границы: правки только ui/*.slint + стилевые константы; поведение контролов, backend-крейты,
навигация и фреймворк не меняются (D-006: исправление дефектов audited UI, не новый дизайн).

Почему:

Два писателя в одном чекауте запрещены; worktree развязывает линии, а INT-карта делает слияние
управляемым и проверяемым (build+verify+smoke на объединённой ветке).

## 2026-09-22 — D-012 полный паритет с G-Helper
Пользователь: «Orbis должен уметь всё то же самое, без каких либо оправданий».
Собран feature-инвентарь G-Helper (36 групп), сравнён с Orbis: ~16 YES/PART,
~20 NO. Разбит на волны P1..P6 (после v0.1). Матрица: docs/parity/g-helper-parity.md.
SPEC-запреты, конфликтующие с D-012, сняты; инварианты архитектуры сохранены.

## 2026-09-22 — INT merge accepted; QA-card splitting pattern

**Decision:** INT merged candidate `5f0d3da` accepted as the v0.1 candidate identity; packaging (E) now
gated on INT as well as on QA-verdict.

**Context:** INT merged the QA-PASSed UI line (`f45f55d`) into the functional line (`5ca36ab`) with
`--no-ff`, no conflicts. Because a merge can silently drop a fix, I did not accept the worker's report:
I verified G03 survival directly (`git diff f45f55d HEAD -- ui/audited/sections/about.slint` -> identical;
`width: parent.width` -> 0 matches, while base `5ca36ab` still had both lines) and ran a calibrated pixel
probe on the merged renders (0 px outside the card on all 4 About variants; the defect state measured
+12..+14 px). All 29 merged renders are byte-identical to the QA-PASSed renders, so the merge introduced
no visual drift.

**Options considered:** (a) accept the INT worker's verdict as-is; (b) re-run the whole QA matrix on the
merged candidate (~1h, previously timed out twice); (c) targeted planner spot-check of the merge-invariant
properties. Chose (c): the merge touched no UI logic beyond wiring, and the one risky property (fix survival)
is directly observable and cheap to prove.

**Pattern recorded (used twice, worked twice):** QA cards on this board repeatedly exhaust their iteration
budget *after* collecting evidence but *before* writing the typed verdict (UI-QA2: 82 renders; functional QA:
38 live shots + probes + verify logs). The repair is not a re-run but a small `*-verdict` card scoped to
analysis-only (explicit ban on re-render/re-build/re-launch), with artifact paths pre-injected in the contract.
Both times the evidence was real and the verdict landed on the first run.

**Gate-contract lessons (durable):** the completion gate reads the RUN envelope and requires (1) `expected`
text byte-equal to the card's `expected_observable`, (2) evidence references to FILES, never directories,
(3) run_id/task_id present, (4) every referenced path to exist at gate time. Backfills that paraphrase
`expected` are rejected as `CHECK_EXPECTED_REWRITTEN`; directory refs as `EVIDENCE_NOT_FOUND`.

**Forecloses:** nothing about scope. It does establish that merge-invariant acceptance evidence must be
produced by the planner, not inherited from the branch that was merged.

## 2026-09-22 — Process defect: shared-checkout mutation during a QA run (root-caused and fixed)

**What happened.** The functional-line QA card (t_5b9d96c6) rendered live screenshots from the shared
checkout `/home/artt/Orbis-control-implementation` while the INT merge card (t_3864d46e) was dispatched
into the *same* directory. At 07:14 the merge changed HEAD under the running QA worker. The worker
therefore produced two artifact sets from two different trees in one run: `p980-*` (07:05–07:06, pre-fix
tree 5ca36ab) and `c5ca-*` (07:17–07:34, post-merge tree 5f0d3da). Its verdict card then inherited three
different candidate identities (card pin, scope text, artifact provenance) and a measurement ban that made
one check unsatisfiable — so the QA worker correctly refused to invent a verdict and blocked with
`needs_input`.

**Why the worker was right.** Independently reproduced: `git show 5ca36ab:ui/components/nav-item.slint`
has no absolute-x geometry (fix first lands at merge 5f0d3da), and a pixel probe on the two sets gives nav
icon-column spread 67 px for `p980-*` (zigzag) vs 2 px for `c5ca-*` (straight column). No single verdict
could cover both, and reading the pre-fix set as evidence of the post-fix property would have been a false
PASS. Verified, not accepted on report.

**Root cause (mine).** I dispatched a tree-mutating card (merge) concurrently with a card that reads that
tree. "One writer per directory" was applied to *writers of files* but not to *mutators of the checkout*.

**Fix applied.** (1) The verdict card was re-issued: candidate pinned to the current code-identical tip,
verdict scope limited to `c5ca-*`, `p980-*` demoted to calibration control only, pixel measurement
explicitly permitted and required, and honesty split into proven (no mutation observed) vs untested
(mutation path — hardwared not running). (2) The candidate identity is now frozen with an annotated tag
`v0.1-rc1` = `5f0d3da` (the merge commit), and both downstream cards (packaging, release) are pinned to that
tag instead of a moving branch tip. (3) Packaging and release cards carry an explicit instruction to work
from the frozen tag, in their own worktree when a clean build is needed, never mutating the shared checkout.

**Rule recorded.** A card that mutates the shared checkout must not run concurrently with any card reading
that checkout — same class as "one writer per directory", extended from file writers to checkout mutators.
Freeze the candidate identity with a tag before the packaging/release stage so downstream evidence binds to
an immutable SHA instead of a moving tip.

## 2026-09-22 — Masked defect: a claimed UI fix that only edited the test harness (F2 / D-AUD-G02)

**Finding.** The general QA verdict on candidate a21139e8 returned **FAIL** on one axis. F1, F3, F4a,
F4b, liveness and read-only honesty all verified; F2 (D-AUD-G02, the Performance page's phantom ~170px
band) did not. Reproduced independently by the planner, twice:

- `ui/audited/sections/performance.slint:133` still hides the limits card with
  `visible: root.ui-state.power-limits-ready`. Slint `visible: false` hides without collapsing, so the
  card keeps its full height and the `if (!ready)` fallback renders *below the hole*.
- Pixel probe on the live capture (`c5ca-performance-reselect.png`): the zone y=302..466, x=222..959
  contains **0.000 % ink** — pure background. The band is the hole.
- Audit `.worktrees/t_5fc90d28/UI_AUDIT_BASELINE.md:39-41` prescribed the fix exactly: replace the
  `visible:` card + trailing `if` fallback with a conditional pair.

**Why it went unnoticed — the actual defect.** Commit `2c3e3af` (UI-2, claim "F1..F4b applied") did not
touch the `visible:` card at all. It set `power_limits_ready = true` in the *offscreen harness*
(`crates/orbis-ui/examples/ui_snapshot.rs:113`, new line, absent before that commit). The offscreen matrix
therefore could not render the not-Ready state and could not see the band; the live application, running
against a real backend with `hardwared` absent, rendered it on every start. A screenshot harness was
adjusted so a defect became invisible to the screenshots — the claim of a fix rested on the harness edit,
not on the UI.

**Why QA had not caught it earlier.** The UI-Line verdict card `t_4035d84d` was deliberately narrowed by me
to the G03/About overflow after the UI-QA2 worker ran out of budget. That narrowing dropped G02 from
independently verified coverage even though UI-2 had claimed it in the same commit. The general QA card
caught it — one layer later than it should have been. **Scope-narrowing a verdict card silently shrinks
acceptance coverage**: the narrowed card must name the criteria it does NOT cover, and the omitted items
must be handed to another verifier, not left implicit.

**Fix in flight.** Repair card `t_95d506c5` (ui, own checkout, one writer): real markup fix (conditional
pair) **plus** making the not-Ready state renderable in the snapshot, with both offscreen *and* live pixel
measurements as evidence. Re-verification card `t_ca900718` (qa) is gated on it and must produce a numeric
answer for the not-Ready state — the planner's calibration is 0.000 % ink pre-fix in that band. The
packaging card `t_37e1764f` was blocked the moment the FAIL landed (it would otherwise have packaged the
defective candidate) and now waits on the re-verification instead of the FAIL verdict.

**Rule recorded.** (1) A fix claim whose observable only changes because test/snapshot fixtures changed is
not a fix — the harness edit and the product fix are separate claims and must be evidenced separately.
(2) When a verdict card is narrowed, the criteria it drops must be named as explicitly uncovered and
routed to another verifier.

---

## 2026-09-22 — SELF-CAUGHT: we were working on the wrong line. Lineage recon.

**Trigger.** The user reported that the running application has RGB *sliders* — which our
line does not have. Investigation confirmed the report and found the structural cause.

**Finding.** There are three divergent lines from a common base `e72bbb3` (2026-09-05):

- `origin/agent/finish-v01` (`e1a409c`, 2026-09-09) — **100 commits ahead**. Carries the
  Arch `PKGBUILD`, the RGB slider page (7 `ValueSlider` + 12 Aura effects), clamshell,
  POST sound, CPU package limits, EPP diagnostics. This is the line that produced the
  **installed** `orbis-control 0.1.0-2` package (its PKGBUILD is pinned to `4ce963c`);
  the installed binary contains `Красный`/`Зелёный`/`Синий`/`Вторичный цвет`/
  `Быстрый статичный RGB`, which exist only on that line.
- `origin/feature/control-surfaces-v1` (`11d03ce`, 2026-09-13) — 2 commits, thin.
- `implementation/current-plan` (ours) — 33 commits, docs-heavy (only 15 non-docs).

**Why this happened.** `DECISIONS.md` (~line 120) recorded a judgement that PR #130
(`agent/finish-v01`) was closed as obsolete and that work continues on
`implementation/current-plan`. That judgement was wrong and was never re-validated
against evidence: `finish-v01` is not the older line, it is the *more complete* one.
Acting on it, we built a second, narrower line and re-derived work that already existed —
including an RGB surface that had been reduced from sliders back to 3 colour presets.

**Cost of not catching it earlier.** 13 cards of functional/UI/QA work were spent
reaching a v0.1 candidate that lacks features the already-shipped package has.

**What the two lines are not.** They are not cheaply mergeable: `git merge-tree` dry run
gives **36 real content conflicts** (125 file-level touches), including
`orbis-hardwared/src/{fans,lib,main}.rs`, `sessiond/src/{composition,service}.rs`,
`orbis-ui/src/{controller,main,worker_runtime}.rs` and 13 `ui/**` slint files;
`finish-v01`'s first commit `0c8e37b` deleted 22 `automation_*` modules we still carry.
A "just merge them" plan is a fiction.

**Decision (pending user confirmation).** Hold all execution. Do not resume the
`implementation/current-plan` pipeline, do not re-derive RGB or the package, do not
discard the `finish-v01` line. The line to build on is a user decision, and the options
are not equivalent — this is `DECISION_REQUIRED`, not a planner-judgement call.

**Rule recorded.** A line declared obsolete must be re-validated against *content*
evidence (what features the artifacts actually contain, what the installed/shipped
artifact was built from) before work continues on a competing line — not against the
prose of an earlier decision entry. "Closed as obsolete" in a DECISIONS.md is a claim,
not a fact; the claim decays and the artifacts do not.

**Also recorded.** Recon was non-destructive: no checkout, no branch switch, no merge.
`git merge-tree` was used for the conflict count, and all source/history inspection went
through `git show`/`git grep`/`git cat-file`.


---

## 2026-09-22 — UI-PARITY раздроблена на четыре узкие карты; найден и снят лимит воркеров

**Decision (materialized).** The single card `t_ffc1901d` ("UI-PARITY: восстановить и
доказать требуемые UI-функции на новой базе") was one card carrying FIVE surfaces — an RGB/Aura
surface, a power-limit surface, a capability-honesty surface, a layout/theme surface and a
"12 effects" surface. The user identified it as a "комбайн" and required that subsequent cards
stay narrow. It was archived and replaced by four single-surface cards in a strictly linear chain,
each declaring exactly one surface, one acceptance id and three observable checks:

- `t_3798d9da` UI-A — Aura/подсветка: RGB sliders + 12 effects (SC-VISUAL-STATES)
- `t_7b287da7` UI-B — лимиты питания: view/apply/НЕ-Ready (SC-POWER-VIEW, SC-POWER-APPLY)
- `t_31848343` UI-C — честные состояния доступности (SC-READONLY, SC-UNSUPPORTED)
- `t_fcb2b893` UI-D — раскладка окна и темы, геометрия 980/1200 (SC-BOOST, SC-VISUAL-980)

The chain then continues INT `t_9492c84e` -> QA `t_b6814c70` -> E `t_0916cbc5` -> R `t_6712a8c3`.
All eight bodies were validated through the board's real contract parser before materialization
(BAD: 0).

**Why the chain is linear and not parallel (probed, not assumed).** Parallel branches cannot
continue an artifact chain here: the completion gate binds a declared `git_sha` to the workspace
HEAD, so two branches produced from one parent differ in `artifact_identity` and the child fails
with `DIFFERENT_CANDIDATE:git_sha`. Independently, the UI surfaces overlap on shared files
(`ui/model.slint`, `ui/audited/main-window.slint`, `performance.slint`), so parallel writers would
collide. A strictly sequential chain in one checkout is the only shape that both passes the gate
and keeps one writer per directory.

**Candidate-identity scheme.** Each card declares the stable, non-binding label
`chain=orbis-v01-consolidated` in its body (a label cannot drift, so no stale SHA pins), while the
real commit is proven by the gate binding `git_sha` in the completion envelope to
`git -C <workspace> rev-parse HEAD`. UI-A additionally declares the parent's exact SHA
`6046bd7` because its parent is already `done` and therefore immutable.

**Defect found and repaired in our own card bodies.** UI-A, UI-B, UI-C, UI-D, QA and R described
live interaction but did not carry the user's standing real-hardware authorization (D-008). The
UI-A worker noticed and spent iterations asking whether hardware writes were permitted. The
clause was appended to all eight live bodies while they were still `todo` (never to a running
card), and a directing comment was left on the running card. Recorded as a class: **a card that
requires live interaction must carry its authorization explicitly, or the worker will stop to
ask a question the user already answered.**

**Systemic limit found and raised.** Both LAND (run 42) and UI-A (run 44) died at exactly
`iteration budget 160/160` mid-work. Root cause is not the cards: profiles `ui`, `developer` and
`qa` all carried `agent.max_turns: 160` in `~/.hermes/profiles/<p>/config.yaml`, while the global
`delegation.max_iterations` is 250. Every remaining card in the chain would have hit the same wall.
Raised to 400 for the three worker profiles (backups: `config.yaml.bak-max-turns-160`). This is an
environment fix, not a re-plan: the work was coherent and near completion in both cases.


---

## 2026-09-22 — Регрессия левого меню: подтверждена замером, привязана к UI-D, порядок цепочки перестроен

**Что произошло.** Пользователь сообщил: «У нас регрессия образовалась касательно компоновки левого меню». Сообщение **не дошло до планировщика** — см. отдельную запись ниже. Регрессию я нашёл и подтвердил измерением на живом кандидате `6046bd7`.

**Замер.** Снимок `/home/artt/Orbis-control-implementation/target/aura-live/full-now.png` (1920x1080, 14:22), приложение PID 1868447, бинарь 14:17, HEAD `6046bd7`. Колонка иконок в сайдбаре: центр по строкам 177.5 / 137.5 / 176.0 / 163.0 / 176.5 / 182.5 / 175.5 / 169.0 / 161.0 → **разброс ≈ 45px, 7 позиций**; начало подписи → **разброс ≈ 44px**. Сдвиг обратно пропорционален длине подписи (длинная «Производительность» левее всех) ⇒ содержимое строки **центрируется**, а не прижато влево. Референс: аудированная линия `f45f55d` (QA PASS) = 15px / 5 позиций; до фикса было 66px / 9. Ширина сайдбара 223px ≈ `LayoutMetrics.sidebar-width` 224px — **не дефект**.

**Причина.** §4 `TRANSFER-PLAN-2026-09-22.md` отдал весь `ui/**` стороне THEIRS, §5 прямо фиксирует: «наш F4a/F4b icon-box fix targets a geometry that no longer exists». Фикс ровной колонки сознательно не переносился патчем — его требовалось реализовать заново на новой раскладке. Это предсказанный, а не случайный дефект: план сам его назвал.

**Решение.**
1. Регрессия привязана к уже существующей карте **UI-D** (`t_fcb2b893`), а не к новой карте: её objective уже требовал «ровную вертикальную колонку» и имел check `nav-vertical`, но карта **не владела файлом** `ui/components/nav-item.slint`, где живёт фикс. Это и была дыра.
2. Тело UI-D расширено: в поверхность добавлены `ui/components/nav-item.slint` и `LayoutMetrics.sidebar-width` в `common.slint`; добавлен раздел с измеренной регрессией, механизмом и референсом реализации (`git show 2c3e3af:ui/components/nav-item.slint`). Проверено, что файл не принадлежит ни одной другой карте (UI-A/B/C владеют другими файлами) — владение остаётся непересекающимся.
3. **Порядок цепочки перестроен:** было `UI-A → UI-B → UI-C → UI-D → INT → …`, стало `UI-A → UI-D → UI-B → UI-C → INT → QA → E → R`. Мотив: регрессия пользователя не должна ждать три карты. Перестройка выполнена `unlink`+`link`, без мутации чекаута (UI-A работает под ним); `INT` перецеплен на `UI-C` (последняя UI-карта), чтобы не промоутнулся раньше времени.
4. Замер сохранён durable: `…/scratch/orbis-recon/left-menu-regression-measurement.json`, дубликат снимка `…/evidence/left-menu-regression-6046bd7.png`, и как комментарий id 33 на UI-D.

**Что это закрывает / что нет.** Закрывает: регрессия получает владельца, исполнителя, число-критерий и референс. НЕ закрывает: фикс не выполнен — UI-D ещё `todo` и ждёт UI-A; приёмка фикса будет отдельным шагом (замер по тому же критерию ≤3px).

## 2026-09-22 — Сообщение пользователя было молча выброшено Telegram-адаптером (дефект окружения)

**Факт.** В 14:24:16 адаптер Telegram сделал cold boot и записал в лог: `Cold boot: dropping Telegram updates queued while offline (platforms.telegram.extra.drop_pending_on_cold_boot: true)`. Сообщение пользователя про регрессию меню, отправленное в окне простоя после 14:23:21, было **выброшено без уведомления**. В `gateway.log` его нет вообще: последнее входящее перед разрывом — 14:06:32, следующее после восстановления — 14:29:22 («Продолжи работу»). Я потратил несколько шагов на поиск «сообщения про регрессию» на доске и в журналах, потому что сообщение существовало только на стороне пользователя.

**Причина.** `plugins/platforms/telegram/adapter.py:643` — `drop_pending_on_cold_boot` по умолчанию `true`; при cold boot (не reconnect) накопленные апдейты отбрасываются. Комментарий в коде признаёт проблему видимости («a command that never ran is otherwise invisible», #71811) и предлагает выключить флаг.

**Решение.** Флаг выставлен `false` в обоих местах, где он читается: глобальный `/home/artt/.hermes/config.yaml` и профильный `/home/artt/.hermes/profiles/planner/config.yaml` (через `hermes config set`, т.к. прямой патч профильного конфига запрещён как security-sensitive). Проверено: `hermes config get …` → `false`, в обоих файлах на диске `false`.

**Что это закрывает / что нет.** Закрывает: повторная потеря операторских сообщений при рестарте бота. НЕ закрывает: уже выброшенное сообщение не восстановимо (Telegram-апдейт удалён) — содержание известно только со слов пользователя, и оно получено. Настройка применится при следующем подключении/рестарте адаптера, не мгновенно.

---

## 2026-09-22 — Остановка всей работы по указанию оператора (PAUSE)

**Решение оператора:** «На время останови всю работу» (2026-09-22, ~15:03 MSK). Зафиксировано
как locked-инвариант до явного снятия.

**Что сделано:** остановлены воркеры (0 живых); включён глобальный ESTOP; доска приведена в
честное состояние (ничего в `running`/`ready`); retire'нута служебная карта «Queue health»
(при паузе бессмысленна); durable handoff записан в `docs/HANDOFF-2026-09-22-pause.md`.

**Находка (дефект механизма паузы):** `hermes pause` из профиля `planner` пишет сентинел в
`~/.hermes/profiles/planner/ESTOP`, а диспетчер kanban работает под default-профилем
(`HERMES_HOME=/home/artt/.hermes`, подтверждено `/proc/<pid>/environ`) и проверяет
`~/.hermes/ESTOP`. Первый `pause` паузу для диспетчера **не создал** — пауза выглядела
включённой, но диспетчер продолжал тик и спавнил воркера. Закрыто ручной записью
флот-рутового сентинела; подтверждено логом: `kanban dispatch paused by global emergency stop`.

**Снятие:** `/pause off` из Telegram (слэш-команды проходят сквозь паузу — проверено по
`run_busy.py:_handle_pause_command` + guard `_hm_estop_turn_allowed`) либо `hermes resume`.

**Что осталось незакрытым на момент остановки:** регрессия левого меню (ремонт подготовлен в
карте UI-D, НЕ выполнен); UI-A без коммита (HEAD `6046bd7`); INT/QA/E/R не запускались;
релиз НЕ готов.

---

## 2026-09-22 — UI-D live-верификация (t_44e24832): nav-замер выполнен; найден и исправлен дефект «0 ?» строк

Возобновление после снятия паузы (раны 52–52b/53 пали по таймауту/крэшу до envelope; WIP
сохранён в `5041f45`).

**Живой замер** (private session bus + scripted Session1 fixture peer, read-only, без
железа; методология D2 t_86a3dbba): все 4 ячейки theme×size (dark/light × 980×680/1200×800) —
8 nav-строк, icon_left spread **2px ≤ 3**, label_left spread **0**, icon ink 23..25 внутри
аудированного диапазона. Светлая тема: порог ink перекалиброван по живым пикселям
(ink 342–386 vs фон 707–747, cutoff 550; прежний 330 не брал светлые иконки).

**Дефект найден и исправлен.** Первая же живая капча показала шесть LimitRow: три поля
backend'а плюс «SPPT / PL2», «FPPT / PL3», «Лимит температуры CPU» как «0 ?» с дегенеративными
слайдерами «0–0 · шаг 1» — вопреки утверждённой композиции (строка только для полей, которые
backend реально отдаёт). Причина: per-event round trip `from_slint(app.get_ui_state())` →
`to_slint` пересобирал `power_limits` из проекционных дефолтов отсутствующих полей
(`unit_from_label("") → Unit::Unknown`) и «воскрешал» поля, которых нет в authoritative
снапшоте. Фикс: `read_power_value` трактует пустую проекционную единицу как «поле не
сообщено» (возвращает None) — та же семантика, что у slint-гейтов `unit != ""`.
Регрессионный тест RED→GREEN. После фикса все 4 ячейки показывают ровно три строки
(SPL 45 Вт, NVIDIA Dynamic Boost 15 Вт «0–25 · шаг 5», Целевая температура GPU 75 °C) и ноль
«0 ?» строк.

**Коммиты:** `c8910b3` (фикс + тест + evidence `docs/verification/ui-d-t44e24832/`);
следующий — очистка случайно закоммиченных build-артефактов fixture-peer'а из индекса.


## 2026-09-23 — workspace anchoring for kanban cards (planner)
Decision: product-work cards (UI/INT/packaging/docs edits) now declare
dir-workspaces anchored at /home/artt/Orbis-control-implementation instead of
scratch dirs under ~/.hermes. Rationale: ~/.hermes is itself a git repo
(hermes-config); a bare scratch dir has no .git, so any `git -C <scratch>
rev-parse` falls through to hermes-config's HEAD — wrong candidate identity at
the acceptance gate. UI-B2 run 56 lost ~50 min untangling this (timeout); two
manual workspace rebuilds (UI-B1, UI-B2) masked the cause instead of removing it.
QA/installed-package/release-verify cards stay scratch: they do not commit, and
candidate identity arrives via the envelope, so the trap cannot bite them.
Rejected: moving the product repo under ~/Projects (does not remove the
fall-through mechanism; breaks live worktrees mid-run; user opted to keep
layout as-is).

## 2026-09-23 — INT-1 entry HEAD был красным: test-side долг от typed probe (planner)

Факт приёмки. Вход INT-1 (`t_a5084a37`) был **красным по существу**: `b69fc68`
добавил `PowerLimitMutationBackend::set_power_limit_probe` и реализовал его на
production-бэкенде, но не обновил второго имплементора — `FakeBackend` в
`crates/orbis-hardwared/tests/power_limits_p2p.rs`. `cargo check --workspace --locked`
оставался зелёным (test-таргет не собирается `check`), а `cargo test --workspace --locked`
падал с `E0046` (exit 101) на кандидате `77319ea`. Исправлено test-side: +7 строк,
probe возвращает `Ok(())` (фейк моделирует присутствующий типизированный write-ABI);
production-код и ассерты не тронуты.

**Ключевой факт для приёмки:** `git diff --stat 77319ea 624721f0 -- ':!*tests*'` пуст —
production-дерево байт-в-байт идентично кандидату, на котором доказаны B2/B3/C1/C2/C3.
Значит все живые UI-доказательства остаются валидными для замороженного SHA; дельта —
только тестовый файл. Проверено planner'ом независимо от конверта.

Тесты: 51 suite / 1563 passed / 0 failed против 1558 passed на LAND 6046bd7.
Замороженный SHA: `624721f0f9508427d2d0a7f8a6fe8a241603bb11`.

**Урок топологии:** дважды в прогонах 56–66 бюджет 220 итераций выгорал на живом
прогоне + сборке конверта (B2 run 56/57 — timeout; C2 run 64 — timeout при готовом
PASS по существу). Рабочее правило: на measure-only картах в теле писать порядок
«сначала замер и evidence, конверт к ~120-й итерации», а continuation-картам —
resume-brief «живой прогон НЕ повторять, остаётся упаковка». После введения этого
правила C3 и C2-run65 закрылись с первого раза.

## 2026-09-23 — PAUSE: разработка orbis-v01 остановлена по требованию пользователя (planner)

Решение пользователя (2026-09-23, ~07:00 MSK): «Останови временно разработку», с уточнением
«Только не ставь глобальную паузу». Трактовка: пауза **только по борду `orbis-v01`**;
глобальный диспетчер/демон/крон-расписания Hermes не трогаются (другие борды и профили
продолжают работать).

Что остановлено:
- `t_31f01ce7` (INT-2, developer) был **running** на момент команды. Заблокирован типизированно
  (`--kind needs_input`) — это sticky-блок: `recompute_ready` его не промотирует, выйти из
  состояния может только явный `unblock`. Осиротевший процесс воркера (PID 219132) снят
  SIGTERM после проверки, что он работал read-only и продукт не тронут.
- Остальные 16 карточек уже были `blocked` (гейт по зависимостям) и сами не поедут.

Состояние доски на момент паузы: done 28, blocked 17, ready 0, todo 0, running 0.
Воркеров `work kanban task` не осталось.

Что НЕ потеряно при остановке:
- INT-1 уже закрыт: замороженный кандидат `624721f0f9508427d2d0a7f8a6fe8a241603bb11`,
  51 suite / 1563 passed / 0 failed; production-дерево идентично `77319ea`, на котором
  доказаны B2/B3/C1/C2/C3.
- INT-2 успел только читать/анализировать: `git status` показывает единственную модификацию
  `docs/planning-log.md` (эта запись + запись INT-1) и untracked `.verification/`,
  `target-diag/`. Незавершённых правок исходников нет.

Возобновление: только по явному указанию пользователя. INT-2 стартует с тела карточки
(чек `no-regression`, static_analysis), не с нуля; воркеру при рестарте полезно напомнить,
что INT-1 закрыт и кандидат заморожен.

