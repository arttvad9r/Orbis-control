# Verification Contract — Orbis Control

> Роль: **VERIFICATION CONTRACT**. Этот документ определяет, какие evidence
> нужны для claims о проверке task, feature или milestone. Он не заменяет
> `AGENTS.md`, `architecture.md`, `current-state.md` или `roadmap.md`.

## 1. Основной принцип

Verification status основывается на фактически сохранённом evidence, а не на
заявлении агента. Фраза «tests pass» без команды, exit status и применимого
контекста сама по себе не является evidence.

Требуемый объём проверки выбирается по правилу:

```text
verification effort ∝ scope + risk + affected boundary
```

Изменение README и изменение privileged hardware write path не имеют одинакового
verification burden. Нельзя ослаблять correctness или safety checks только ради
получения зелёного результата.

Architecture boundaries и hardware invariants определяются в
[`architecture.md`](architecture.md) и корневом [`AGENTS.md`](../AGENTS.md).
Фактический baseline находится в [`current-state.md`](current-state.md), а
порядок будущих работ — в [`roadmap.md`](roadmap.md).

## 2. Verification claims

Claims описывают, какой тип утверждения поддержан evidence. Это независимые
измерения, а не обязательная линейная лестница: `MANUALLY_ACCEPTED` не является
автоматически уровнем выше `HARDWARE_VERIFIED`, и отдельной task могут требоваться
только некоторые claims.

| Canonical claim | Что означает |
|---|---|
| `IMPLEMENTATION_PRESENT` | Требуемая реализация существует в коде или config; correctness не доказана |
| `STATIC_BUILD_VERIFIED` | Применимые format, compile, build или static checks имеют успешное evidence |
| `TESTED` | Применимые automated tests имеют успешное evidence |
| `RUNTIME_VERIFIED` | Требуемое runtime behaviour реально наблюдалось в указанной environment |
| `HARDWARE_VERIFIED` | Требуемое поведение проверено на соответствующем физическом device |
| `MANUALLY_ACCEPTED` | Человек явно принял результат там, где automation недостаточна |

Существующее слово `IMPLEMENTED` в [`current-state.md`](current-state.md) — это
project-status terminology. Оно не является canonical evidence claim и не
заменяется массовой миграцией старых документов.

`MOCK-ONLY`, `UNKNOWN`, `PARTIAL` и `NOT IMPLEMENTED` остаются project status
terms из `current-state.md`, а не заменяют claims. Особенно mock/test backend не
поддерживает `HARDWARE_VERIFIED`.

## 3. Check/result status

Status относится к результату конкретной verification check, а не к claim.

| Status | Что означает |
|---|---|
| `PASS` | Check выполнена, и требуемое условие подтверждено |
| `FAIL` | Check выполнена, но требуемое условие не подтверждено |
| `DEFERRED` | Check требуется, но сознательно перенесена; обязателен `reason` |
| `BLOCKED` | Check невозможно завершить из-за blocker; обязателен blocker/reason |
| `REQUIRES_USER` | Нужны user-only action, physical device, visual assessment или другое человеческое участие; это не `PASS` |
| `NOT_RUN` | Check ещё не выполнялась |

Связь между категориями имеет вид:

```text
task acceptance criteria
        ↓
required checks
        ↓
check status + evidence
        ↓
supported verification claims
```

Например, `cargo test` со статусом `PASS` может поддержать `TESTED`, но
`hardware-check` со статусом `DEFERRED` не поддерживает `HARDWARE_VERIFIED`.
`DEFERRED`, `BLOCKED`, `REQUIRES_USER` и `NOT_RUN` никогда не являются claims.

## 4. Evidence requirements

Для command-based check обычно сохраняются:

- фактическая команда и релевантные аргументы;
- фактический exit status;
- duration;
- короткий summary результата;
- relevant failure excerpt при failure;
- путь к полному log, если он нужен для последующей диагностики;
- target/environment, включая runtime или device context, когда это важно.

Для non-command checks сохраняются наблюдение, target/environment и явная
manual/hardware acceptance. Ссылка на code path, screenshot, raw backend value,
dated probe или D-Bus baseline добавляется, когда она является частью proof.

Evidence должен позволять независимо установить нужные claims, например:

```text
IMPLEMENTATION_PRESENT
STATIC_BUILD_VERIFIED
TESTED
RUNTIME_VERIFIED
HARDWARE_VERIFIED
MANUALLY_ACCEPTED
```

Это не обязательная последовательность и не означает, что каждый claim нужен
каждой task. Неподтверждённый claim нельзя молча считать выполненным.

### Минимальная форма будущего evidence record

Окончательная JSON Schema будет отдельной задачей. Сейчас contract требует
концептуально следующие поля:

```text
check_id
profile
kind
status
command                 optional for non-command checks
exit_code               optional for non-command checks
duration
summary
failure_excerpt         optional
full_log_path           optional
target_or_environment   optional when relevant
reason                  required for DEFERRED/BLOCKED/REQUIRES_USER when applicable
supported_claims        optional
```

`status` должен быть явно классифицирован. Нельзя использовать пустой результат,
отсутствие log или текстовый claim агента как implicit `PASS`.

## 5. Verification profiles

Профиль выбирается по scope и risk конкретной task. Профили — conceptual
группировка проверок; они не реализуют verifier и не устанавливают controller
policy.

### `quick`

Дешёвый feedback для ранней итерации и локализации очевидных ошибок. Он может
включать `git diff --check`, релевантный targeted check или узкий component test.
`quick` не означает final acceptance и не должен заявляться как full project
verification.

### `task`

Минимальный набор проверок, покрывающий acceptance criteria конкретной atomic
task и каждую затронутую boundary. В зависимости от изменения это могут быть:

- docs-only checks;
- format/static/build check;
- crate-specific unit/component tests;
- integration test через private P2P transport;
- runtime/UI observation;
- hardware evidence, если task изменяет hardware-sensitive behaviour.

### `full`

Широкая project/milestone verification для scope и risk, которые её требуют.
Текущий project FULL tier из `AGENTS.md` включает INTEGRATION и, при
необходимости, Nix checks:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
nix build .#orbis-control --max-jobs 1 --cores 4
nix flake check --max-jobs 1 --cores 4
```

Nix commands выполняются только когда scope их действительно требует, например
при milestone/release acceptance или Nix/package/module changes.

## 6. Project-specific checks

Для обычного изменения одного crate authoritative FAST tier из `AGENTS.md`:

```bash
cargo fmt --all -- --check
cargo check -p <affected-crate> --all-targets
cargo test -p <affected-crate>
cargo clippy -p <affected-crate> --all-targets -- -D warnings
git diff --check
```

Для изменений, пересекающих crates или service boundaries, применяется
INTEGRATION tier:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
```

Для документационной task минимальная project policy — `git diff --check` плюс
релевантная проверка ссылок/терминов. Cargo/Nix checks не обязательны, если
изменение не затрагивает код, packaging или Nix.

Категории automated checks отражают реальный проект:

- unit tests для domain/provider/application semantics;
- component/crate-specific checks;
- integration и protocol checks, включая private P2P D-Bus transport;
- runtime/UI checks для packaged или подходящего dev environment;
- live hardware checks только при фактическом обращении к target device.

Не запускать реальные UPower/asusd/supergfxd, system/session bus, hardware writes
или privileged operations без explicit permission конкретной task. Read-only
probe и test fixture не являются write или hardware proof.

## 7. Runtime, hardware и manual evidence

`RUNTIME_VERIFIED` требует запуска и наблюдаемого поведения в указанной среде.
Successful compile, test binary creation или запуск mock path недостаточны.

`HARDWARE_VERIFIED` требует одновременно:

- соответствующего физического device;
- фактического backend/device observation;
- dated или иначе идентифицируемого evidence;
- отсутствия подмены mock/default value вместо hardware fact.

Нельзя повышать `mock test`, unit test, compile или VM execution до
`HARDWARE_VERIFIED`. VM может подтвердить software/protocol/packaging property,
но не реальную semantics конкретного ноутбука.

Для UI/UX, pixel analysis и других checks, где автоматического assertion
недостаточно, используется `MANUALLY ACCEPTED` с описанием того, что именно было
осмотрено. Manual acceptance не стирает `DEFERRED` checks.

Hardware facts и dated probes подчиняются hierarchy из
[`docs/README.md`](README.md): live evidence и fixtures имеют приоритет над
inference, mock values и product defaults. Unknown bounds остаются unknown;
отсутствие evidence не превращается в `Unsupported` или success.

## 8. Deferred, blocked и requires-user

`DEFERRED` используется, когда check известен и нужен, но сейчас объективно не
выполнен. Record должен содержать:

- что именно не проверено;
- почему сейчас это отложено;
- какие environment, device, permission или manual action нужны далее.

`BLOCKED` используется, когда конкретный blocker не позволяет выполнить
обязательную проверку. Нужно записать blocker и условие разблокировки.

Если check требует физический device, live session, privileged permission или
явное человеческое решение, это должно быть обозначено как `REQUIRES_USER` или
как соответствующий `DEFERRED/BLOCKED` result внешнего workflow. Нельзя
симулировать отсутствующее evidence и нельзя автоматически объявлять task
полностью проверенной.

Независимые checks могут продолжаться, если они не требуют заблокированного
условия. Но итог task должен явно сохранять unresolved status.

## 9. Logs и claims

Большие raw logs не передаются reviewer/LLM по умолчанию. Нормальное evidence:

```text
command
exit status
short summary
relevant failure excerpt
path to complete log
```

Полный log читается только для необходимой diagnosis/review. Это не заменяет
сохранение log path или failure excerpt.

Следующие выводы запрещены без соответствующего evidence:

```text
compiles                  → therefore works
unit tests pass           → therefore hardware works
CLI path works            → therefore UI workflow works
mock succeeds             → therefore real device succeeds
agent says PASS           → therefore PASS
```

## 10. Task и milestone completion

Atomic task может считаться complete только когда:

1. implementation соответствует acceptance criteria;
2. обязательные для task checks выполнены;
3. результаты имеют evidence;
4. оставшиеся checks явно классифицированы как `DEFERRED`, `BLOCKED` или
   `REQUIRES_USER`, если это допустимо для данной task;
5. не заявлен более высокий verification level, чем подтверждён evidence.

Milestone может требовать более сильного уровня, чем отдельная task. В Orbis
roadmap milestone обычно требует не только code/tests, но и integration,
packaging, runtime или live hardware evidence согласно его Definition of Done.
Например, `LIVE-VALIDATED` read-only MVP не становится completed только из-за
unit tests, а Milestone 4 остаётся active, пока fan/telemetry и другие pending
substeps не закрыты. Состояние roadmap изменяется только в `roadmap.md` и не
меняется самим verification record.
