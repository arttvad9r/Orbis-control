//! # orbis-hardwared — binary foundation.
//!
//! D-Bus service НЕ реализован: polkit/activation/identity contract остаётся
//! следующему шагу (ADR 0006). Бинарь не выполняется как root и не выполняет
//! никакого sysfs I/O; он только сообщает о foundation-статусе.

fn main() {
    eprintln!("orbis-hardwared: foundation only; D-Bus service не реализован");
}
