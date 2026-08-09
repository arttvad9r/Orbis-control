# ADR 0005: Split GPU provider capabilities by hardware concept

- Статус: **Принято** (2026-08-09)
- Дата: 2026-08-09

## Контекст

Orbis GPU domain уже различает независимые hardware/backend concepts:

- product/requested `GpuMode` (Eco / Standard / Ultimate / Optimized);
- physical MUX state (`GpuMuxState`);
- dGPU access policy (`GpuAccessPolicy`);
- runtime power state (`GpuPowerState`);
- action requirement / pending behavior (`ActionRequirement`).

Но legacy `GpuProvider` объединяет методы всех concepts в один trait.

Это привело к реальной проблеме: `SupergfxdGpuPowerProvider` имеет
authoritative runtime power backend (PROVEN supergfxd `Power()` enum), но не
является источником product `GpuMode`, MUX или access policy. До split он был
вынужден реализовывать чужие методы с `ProviderError::Unsupported`.

Также `AppService::gpu_state()` остаётся fail-fast aggregate
(`requested_mode()? → mux_state()? → access_policy()? → power_state()?`) и не
подходит как единственный API для partially available concepts: при
Unsupported requested mode реальный power через aggregate практически
недоступен.

## Решение

1. GPU backend capabilities могут и должны выражаться отдельными provider
   traits по concept boundaries.

2. Provider реализует только те capabilities, которыми реально владеет; не
   обязан реализовывать несвязанные GPU capabilities.

3. Первый такой trait — `GpuPowerProvider`:

   ```rust
   #[async_trait]
   pub trait GpuPowerProvider: Provider {
       async fn power_state(&self) -> Result<GpuPowerState, ProviderError>;
   }
   ```

   `SupergfxdGpuPowerProvider` реализует `Provider` + `GpuPowerProvider` и
   больше НЕ реализует legacy `GpuProvider` (удалены fake/Unsupported методы
   `requested_mode`/`set_mode`/`mux_state`/`access_policy`/`requirement_for`/
   `validate_mode`).

4. Application layer может иметь независимые capability-specific getters,
   например `AppService<P>::gpu_power_state()` при `P: GpuPowerProvider`,
   который вызывает только `provider.power_state()`.

5. Unsupported capability и Unknown state остаются разными понятиями:

   - capability отсутствует → trait не реализован / Unsupported на boundary,
     где это действительно runtime capability question;
   - capability поддерживается, но semantic state неизвестен →
     `GpuPowerState::Unknown`;
   - read поддерживаемого capability failed → `ProviderError`.

6. Hardware concepts не обязаны происходить из одного backend. Будущая
   composition может использовать разные providers/sources для:
   product/mode policy, MUX, access, runtime power, pending/action.

7. Legacy `GpuProvider` пока сохраняется для существующего full product-mode /
   mutation / aggregate path. НЕ объявлять его немедленно deprecated или
   запланированным к удалению — такого решения пока нет.

8. `GpuState` сейчас НЕ redesign-ить. Aggregate `AppService::gpu_state()`
   остаётся существующим convenience/full path и не является обязательным
   способом чтения каждого concept.

9. Новые concept traits вводятся incremental, только когда соответствующий
   backend/mapping доказан. `GpuMuxProvider`/`GpuAccessProvider` НЕ считаются
   реализованными.

## Последствия

Положительные:

- capability-driven API;
- backend не притворяется источником чужих concepts;
- real partial hardware state можно читать независимо;
- разные backend sources можно композиционировать;
- сохраняется distinction Unknown / Unsupported / Error;
- будущий Session1 может публиковать concepts независимо.

Tradeoffs:

- некоторое время существуют legacy `GpuProvider` и новые concept traits рядом;
- `MockProvider` может реализовывать несколько traits;
- application/session/UI migration будет incremental;
- aggregate `gpu_state()` пока остаётся fail-fast (limitation legacy aggregate,
  не blocker для independent power API).

## Rejected alternatives

A. Только добавить independent `AppService` getters, оставив monolithic
   provider contract: отклонено как долгосрочная архитектура, потому что
   backend всё ещё обязан реализовывать чужие методы.

B. Partial `GpuState` через `Option`/per-field optionality: отклонено сейчас,
   потому что смешивает unsupported capability, unknown semantic state и
   runtime error и создаёт широкую domain/wire/UI migration.

Эти варианты не запрещены навсегда; это решение для текущей архитектуры.
