# Orbis Control — visual refresh from supplied references

## Raw brief (2026-09-24)

> «В загрузке есть папка "Новая папка". Там лежит желательный визуал приложения.
> Нужно реализовать. Не прям пиксель в пиксель, но желательно максимально похоже.
> Несуществующих пунктов делать не надо. Но можно сделать ui сразу на будущее с учётом
> того, какие функции будут в итоге добавлены. Просто оставить их без подключенного
> бэкэнда, типа сделать mock или типа того».

## Locked decisions

- **Scope:** refresh the existing Orbis Control desktop UI; do not add new top-level pages merely because they appear in the references. In particular, no new `Profiles` or `Devices` navigation destinations are created for this slice.
- **Locale:** Russian (`ru-RU`).
- **Reference authority:** `TARGET_DESIGN`. Preserve the observable composition, hierarchy, dark material language, spacing rhythm and control character as closely as the actual Orbis section inventory and runtime states permit. Pixel equality is not required.
- **Future controls:** planned but backend-unwired controls may look and feel interactive. Press, hover, selection and toggle feedback may change session-local visual state only; it resets on navigation or restart. No explanatory preview label is shown. These controls must not emit any backend callback or claim that hardware changed, saved, applied, or was read back.
- **Existing real controls:** retain their current backend wiring and authoritative/readonly/unavailable semantics. This refresh must not substitute mock results for real read-back.

## Reference manifest

The source files were copied without alteration from `/home/artt/Downloads/Новая папка/` into `docs/references/orbis-visual-2026-09-24/` for custody. Screenshots are design evidence, not shipped application assets.

| Screen | Project copy | SHA-256 |
| --- | --- | --- |
| Dashboard | `Изображение Codex 24 сент. 2026 г., 09_40_37-1.png` | `ad34b85c9c8ca73a4a5cca539ee3ee93afe9579bdc14fb62e4c15c61b92859df` |
| Profiles (reference only; no new page) | `Изображение Codex 24 сент. 2026 г., 09_40_38-2.png` | `298342eb5e84626ea48433eed6bc147dd4b531f14f45e5e863955cefaed90f72` |
| Performance | `Изображение Codex 24 сент. 2026 г., 09_40_38-3.png` | `8f4a2e5b8c4ac8fcc3b4016abbfb5f2e34b0be8984f2c806e25e9269975fa793` |
| Cooling | `Изображение Codex 24 сент. 2026 г., 09_40_39-4.png` | `849eaf98f0ab120b656d51526d4a2ba14cb10e70ad6a61a1d95bfc8a7b7a2627` |
| Power | `Изображение Codex 24 сент. 2026 г., 09_40_40-5.png` | `f2634830d159546d833852c4f4f096cf76e75b9c1407db0c6b75362da88491e2` |
| Display | `Изображение Codex 24 сент. 2026 г., 09_40_41-6.png` | `330b3839b94c3ffe072d17a9c9ae60cc671fae932231eb7aaf39702b6723f802` |
| Backlight | `Изображение Codex 24 сент. 2026 г., 09_40_42-7.png` | `b0bf1098e76fd116080798f16deba455c3683f5df6d4ce98a706b011f1095c77` |
| Devices (reference only; no new page) | `Изображение Codex 24 сент. 2026 г., 09_40_43-8.png` | `a16180fbbfa798342681835654dffb93db025d5725cb6de5023e80f27567ae18` |
| System | `Изображение Codex 24 сент. 2026 г., 09_40_44-9.png` | `0609a1e65fd5d3ea3b225191f0435fc83eaee610f8ddc8680ce1febc9077ad2c` |
| Settings | `Изображение Codex 24 сент. 2026 г., 09_40_45-10.png` | `079c2992869eb54d58496c1f08ce44646976bdcd4b757d81b4f51798656187e4` |

## Visual system requirements

1. **Shell:** retain a frameless desktop window and left navigation. Use the reference’s deep blue-charcoal surfaces, one strong blue interaction accent, restrained semantic green/amber/red, thin borders, 10–14 px radii, and calm elevation. Preserve Orbis branding; do not use ASUS/ROG logos or imagery as product assets.
2. **Navigation:** use the existing Orbis destinations: `Главная`, `Производительность`, `Питание`, `Охлаждение`, `Графика`, `Подсветка`, `Экран`, `Система`, `Настройки`, `О программе`. Align icon/text geometry, active fill and hover treatment with the reference language; do not create reference-only destinations.
3. **Pages:** make the existing Dashboard, Performance, Cooling, Power, Display, Backlight, System and Settings sections visually resemble their matching reference compositions where functionally applicable. Use card grids, settings rows, page header hierarchy, segmented selectors, sliders, toggles, pills and navigation cards only where the corresponding Orbis capability exists or is a declared preview/mock.
4. **Capability truth:** real backend values keep their precise Loading, Ready, readonly, unavailable, pending, dirty, error, queued and read-back states. Do not fill a real hardware surface with invented telemetry.
5. **Preview/mock behavior:** a non-wired future control provides normal visual press, hover, selection or toggle feedback only. It may update session-local visual state in the current page/window; navigation away or application restart restores the initial state. It emits no backend callback and may not say `Применено`, `Сохранено`, `Готово`, or equivalent hardware-success language.
6. **Responsive presentation:** preserve usable rendering at the project’s existing 980×680 minimum and 1200×800 comfort sizes, in both dark and light theme modes. The reference constrains the dark visual target; light mode remains coherent rather than copying a dark screenshot literally.

## Interaction additions for preview/mock controls

| Control category | User action | Observable result | Non-result | QA check |
| --- | --- | --- | --- | --- |
| Preview toggle / selector / slider | Click, select or drag | Normal visual pressed/selected/toggled feedback; any state is local to the current page/window | No backend callback, file persistence, Hardware1 mutation or success claim | Refresh page and restart application: initial value returns; inspect no mutation evidence |
| Real capability control | Use existing Orbis interaction | Existing runtime-specific state and read-back behavior remains intact | No replacement with a mock state | Existing regression checks plus UI spot-check |
| Reference-only page | Attempted navigation | No destination/control is present | No invented surface | Navigation inventory matches current `Section` enum |

## Visual acceptance states

Representative review captures must compare `REFERENCE → ACTUAL` for:

- Dashboard at 1200×800 dark;
- Performance with both real-capability and unavailable/read-only layouts;
- Cooling with CPU/GPU selection, dirty/pending/error states where available;
- Power, Display and Backlight with enabled locally interactive preview controls;
- System and Settings card grids;
- both 980×680 and 1200×800 in dark and light;
- no clipping, overlap, horizontal content scrolling, phantom whitespace, or false hardware-success feedback.

## Scope boundary

This is a visual/product presentation refresh. It must not change privileged write contracts, `PowerLimitProvider`, Hardware1 production ABI, session-client composition, or actual device-mutation behavior. Any discovery that requires a new hardware capability returns to Planner rather than being invented in the UI.
