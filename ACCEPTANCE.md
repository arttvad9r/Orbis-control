# Orbis Control — ACCEPTANCE

## AC-001: Запуск на поддерживаемой системе

Given:
- Orbis Control установлен на Arch Linux;
- необходимые services доступны.

When:
- пользователь запускает приложение.

Then:
- GUI запускается без root;
- hardware sections переходят из Loading в фактические состояния;
- отсутствуют бесконечные loading indicators;
- не отображаются фиктивные hardware values.

---

## AC-002: Неподдерживаемая capability

Given:
- конкретная hardware capability отсутствует по runtime evidence.

When:
- пользователь открывает соответствующую страницу.

Then:
- control не позволяет mutation;
- UI показывает Unsupported/«Недоступно» либо скрывает неприменимый control согласно design system;
- приложение не создаёт fake default;
- остальные независимые capabilities страницы продолжают работать.

---

## AC-003: Read-only capability

Given:
- read path поддерживается;
- write path не поддерживается.

When:
- пользователь открывает control.

Then:
- фактическое значение отображается;
- изменение недоступно;
- состояние явно обозначено как Read-only;
- отсутствие write capability не интерпретируется как отсутствие read capability.

---

## AC-010: Переключение performance profile

Given:
- несколько performance profiles доступны;
- mutation writable.

When:
- пользователь выбирает другой profile.

Then:
- выполняется typed mutation;
- выполняется authoritative read-back;
- UI показывает новый profile только после подтверждённого observed state;
- при mismatch requested state не изображается как Applied.

---

## AC-011: Profile уже активен

Given:
- Balanced уже является observed profile.

When:
- пользователь выбирает Balanced.

Then:
- не появляется ложный Pending state;
- итоговый observed state остаётся Balanced;
- операция завершается корректным no-op либо подтверждением текущего состояния согласно backend contract.

---

## AC-020: Редактирование и применение CPU fan curve

Given:
- CPU curve readable и writable.

When:
- пользователь изменяет допустимую точку кривой.

Then:
- hardware не меняется немедленно;
- UI показывает Dirty state.

When:
- пользователь нажимает «Применить».

Then:
- curve проходит validation;
- выполняется требуемая mutation;
- выполняется read-back;
- отображается фактически прочитанная curve;
- Dirty state снимается только после успешного результата.

---

## AC-021: Редактирование GPU fan curve

Given:
- GPU fan curve поддерживается.

When:
- пользователь выбирает GPU и изменяет curve.

Then:
- редактируется именно GPU curve;
- CPU curve не изменяется;
- после Apply read-back подтверждает GPU curve.

---

## AC-022: Невалидная fan curve

Given:
- пользовательский draft нарушает backend/domain constraint.

When:
- пользователь пытается применить curve.

Then:
- hardware mutation не выполняется;
- UI показывает причину validation failure;
- observed curve остаётся прежней;
- draft можно исправить.

---

## AC-023: Несохранённая curve при смене fan/profile

Given:
- текущая curve имеет Dirty state.

When:
- пользователь выбирает другой fan или profile.

Then:
- изменения не теряются молча;
- пользователь получает выбор остаться либо отбросить draft;
- Cancel оставляет текущий editor без hardware mutation;
- Discard восстанавливает observed state и выполняет переход.

---

## AC-024: Factory fan reset

Given:
- backend доказанно поддерживает factory reset.

When:
- пользователь выбирает reset и подтверждает Apply.

Then:
- используется существующая typed reset operation;
- выполняется read-back;
- UI отображает полученную vendor curve.

Given:
- reset capability не доказана.

Then:
- action недоступна.

---

## AC-030: Отображение power limits

Given:
- backend предоставляет SPL, SPPT и FPPT;
- для каждого доступны metadata.

When:
- пользователь открывает Performance.

Then:
- отображаются три отдельных поля;
- каждое показывает корректные value/unit/min/max/step;
- UI не объединяет их в одно вымышленное hardware value.

---

## AC-031: Применение power limit

Given:
- SPL writable;
- backend сообщает допустимый диапазон и step.

When:
- пользователь вводит допустимое новое значение и применяет его.

Then:
- выполняется validation;
- mutation проходит через typed privileged path;
- значение перечитывается;
- UI показывает фактический read-back.

---

## AC-032: Power limit вне диапазона

Given:
- backend сообщает диапазон 20..80.

When:
- пользователь пытается отправить значение 90.

Then:
- mutation не отправляется;
- observed state не меняется;
- UI отображает validation error.

---

## AC-033: Изменение диапазона backend

Given:
- после driver/backend update authoritative maximum отличается от ранее известного.

When:
- приложение получает новый snapshot.

Then:
- UI использует новый backend maximum;
- старое hard-coded ограничение не применяется.

---

## AC-034: Отказ polkit

Given:
- пользователь запрашивает privileged mutation.

When:
- authorization отклонена.

Then:
- приложение выходит из Applying;
- observed value не заменяется requested value;
- отображается authorization error;
- пользователь может повторить действие вручную.

---

## AC-035: Неизвестный исход mutation

Given:
- write могла быть отправлена;
- response завершился timeout или connection loss.

When:
- приложение обрабатывает outcome.

Then:
- mutation автоматически не повторяется;
- состояние обозначается как Unknown/Pending verification;
- выполняется fresh read;
- дальнейший UI state определяется результатом read.

---

## AC-040: NVIDIA Dynamic Boost

Given:
- backend предоставляет `GpuDynamicBoost`.

When:
- пользователь открывает Performance.

Then:
- control показывается отдельно от platform Turbo profile;
- отображаются реальные unit/min/max/step;
- Apply выполняется с read-back.

---

## AC-041: CPU Boost без доказанного backend

Given:
- authoritative CPU boost read/write capability не найдена.

When:
- приложение обнаруживает hardware capabilities.

Then:
- приложение не создаёт работающий CPU Boost toggle;
- platform Turbo не выдаётся за CPU Boost;
- отсутствует shell/sysfs workaround без доказанного contract.

---

## AC-050: GPU mode с reboot requirement

Given:
- пользователь находится в Standard;
- Ultimate доступен;
- переход требует reboot.

When:
- пользователь выбирает Ultimate.

Then:
- backend принимает или ставит transition в очередь;
- UI показывает Ultimate как Queued/Pending;
- текущий physical MUX остаётся observed current value;
- отображается сообщение о reboot/shutdown requirement;
- Pending не подписывается как Applied.

---

## AC-051: GPU mode без фактического изменения

Given:
- current и requested product mode совпадают.

When:
- пользователь выбирает текущий mode.

Then:
- не появляется ложное reboot-required;
- UI остаётся согласованным с backend snapshot.

---

## AC-052: Частичная GPU capability

Given:
- MUX state доступен;
- dGPU runtime power provider недоступен.

When:
- пользователь открывает Graphics.

Then:
- MUX отображается;
- power state отображается как Unavailable;
- отсутствие power provider не скрывает и не ломает MUX controls.

---

## AC-060: Hardware state не восстанавливается из preferences

Given:
- при прошлом запуске пользователь применял custom power/fan value;
- приложение перезапущено;
- реальное hardware state теперь другое.

When:
- приложение загружается.

Then:
- UI отображает свежий observed hardware state;
- loading preferences самостоятельно не выполняет hardware mutation.

---

## AC-070: Backend недоступен при запуске

Given:
- `orbis-sessiond` или `orbis-hardwared` недоступен.

When:
- приложение запускается.

Then:
- GUI не падает;
- соответствующие controls получают честное Unavailable/Error состояние;
- остальные независимые функции продолжают отображаться;
- доступна диагностика причины.

---

## AC-071: Backend пропал во время работы

Given:
- control был Ready.

When:
- соответствующий service становится недоступен.

Then:
- UI не продолжает изображать stale state как подтверждённый writable state;
- следующая операция завершается контролируемой ошибкой;
- приложение не падает.

---

## AC-080: Minimum window layout

Given:
- окно имеет размер 980×680.

When:
- пользователь последовательно открывает Dashboard, Performance, Cooling и Graphics.

Then:
- controls не перекрываются;
- текст не наезжает на кнопки;
- нет horizontal scroll primary content;
- доступный контент можно получить vertical scroll;
- primary actions остаются доступными.

---

## AC-081: Visual regression states

Given:
- snapshot/test fixture environment доступен.

When:
- рендерятся обязательные visual states из `SPEC.md`.

Then:
- каждый snapshot непустой;
- отсутствуют очевидные overlap/clipping;
- dark/light variants рендерятся корректно;
- Dirty/Pending/Error визуально различимы.

---

## AC-090: Регрессия существующих функций

Given:
- существующий backend поддерживает battery charge limit и другие production-ready controls.

When:
- выполняется regression verification после реализации новой версии.

Then:
- существующие working flows по-прежнему проходят tests;
- новая работа не отключает их ради упрощения новых controls.

---

## AC-100: Arch installation smoke

Given:
- чистая поддерживаемая Arch environment;
- собран текущий release artifact.

When:
- выполняется canonical Arch installation.

Then:
- binaries устанавливаются в пути, совпадающие с systemd units;
- D-Bus/polkit assets установлены;
- services запускаются;
- GUI запускается обычным пользователем;
- root GUI не требуется.

---

## AC-110: Verification gate

Given:
- текущий implementation stage завершён.

When:
- выполняются проверки из `IMPLEMENTATION_PLAN.md`.

Then:
- агент сообщает фактически выполненные команды и результаты;
- failing check не обозначается как success;
- недоступный check явно отмечается как непроверенный;
- «готово» не заявляется только на основании анализа исходников.
