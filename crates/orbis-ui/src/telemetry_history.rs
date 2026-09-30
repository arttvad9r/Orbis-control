//! Rolling telemetry history for the dashboard sparklines and the
//! performance temperature chart. Samples come from the same authoritative
//! telemetry refresh that feeds the numeric readouts; a stale or missing
//! reading is recorded as a gap, never as a value.

use std::cell::RefCell;
use std::time::Instant;

use orbis_ui::sparkline::{self, CHART, Range, SPARK, Series, leading_number};

use crate::AppWindow;
use crate::controller;

#[derive(Default)]
struct History {
    cpu_temp: Series,
    gpu_temp: Series,
    battery: Series,
    power: Series,
    cpu_fan: Series,
    gpu_fan: Series,
    started: Option<Instant>,
}

thread_local! {
    static HISTORY: RefCell<History> = RefCell::new(History::default());
}

fn known(value: i32) -> Option<i32> {
    (value >= 0).then_some(value)
}

/// Append the current telemetry readings and republish the chart images.
pub fn record(app: &AppWindow, state: &controller::UiState) {
    HISTORY.with(|cell| {
        let mut history = cell.borrow_mut();
        let fresh = state.telemetry_fresh;
        let sample = |value: Option<i32>| if fresh { value } else { None };
        // A sleeping dGPU reports nothing; the awake iGPU carries the trend.
        let gpu_temp = known(state.gpu_temp_value).or(known(state.igpu_temp_value));
        let power = leading_number(&state.power_ac)
            .or_else(|| leading_number(&state.gpu_power_display))
            .or_else(|| leading_number(&state.igpu_power_display));
        history.cpu_temp.push(sample(known(state.cpu_temp_value)));
        history.gpu_temp.push(sample(gpu_temp));
        history
            .battery
            .push(sample(known(state.battery_percent_value)));
        history.power.push(sample(power));
        history
            .cpu_fan
            .push(sample(leading_number(&state.cpu_fan_rpm)));
        history
            .gpu_fan
            .push(sample(leading_number(&state.gpu_fan_rpm)));
        history.started.get_or_insert_with(Instant::now);
        app.set_cpu_fan_level(fan_level(&state.cpu_fan_rpm));
        app.set_battery_health_value(leading_number(&state.battery_health).unwrap_or(-1));
        app.set_gpu_fan_level(fan_level(&state.gpu_fan_rpm));
        publish(app, &history);
    });
}

/// Laptop fans top out around 6000 rpm; the bar is a coarse level only.
fn fan_level(rpm: &str) -> f32 {
    leading_number(rpm).map_or(-1.0, |rpm| (rpm as f32 / 6000.0).min(1.0))
}

fn image(series: &Series, range: Range, canvas: sparkline::Canvas) -> slint::Image {
    sparkline::line_chart_svg(&series.recent_values(), range, canvas)
        .and_then(|svg| slint::Image::load_from_svg_data(svg.as_bytes()).ok())
        .unwrap_or_default()
}

fn publish(app: &AppWindow, history: &History) {
    let temp = Range::Auto { min_span: 8.0 };
    app.set_cpu_temp_spark(image(&history.cpu_temp, temp, SPARK));
    app.set_gpu_temp_spark(image(&history.gpu_temp, temp, SPARK));
    app.set_battery_spark(image(
        &history.battery,
        Range::Auto { min_span: 4.0 },
        SPARK,
    ));
    app.set_power_spark(image(&history.power, Range::Auto { min_span: 6.0 }, SPARK));
    app.set_cpu_fan_spark(image(
        &history.cpu_fan,
        Range::Auto { min_span: 600.0 },
        SPARK,
    ));
    app.set_gpu_fan_spark(image(
        &history.gpu_fan,
        Range::Auto { min_span: 600.0 },
        SPARK,
    ));
    let axis = Range::Fixed {
        min: 20.0,
        max: 100.0,
    };
    app.set_cpu_temp_chart(image(&history.cpu_temp, axis, CHART));
    app.set_gpu_temp_chart(image(&history.gpu_temp, axis, CHART));
    let seconds = history.started.map_or(0, |start| start.elapsed().as_secs());
    app.set_history_span(span_label(seconds, history.cpu_temp.len()).into());
}

/// Label for the chart's left edge: how far back the full-width window
/// reaches at the observed poll rate (series grow in from the right).
fn span_label(elapsed_secs: u64, samples: usize) -> String {
    if samples < 2 {
        return String::new();
    }
    let per_sample = elapsed_secs as f64 / (samples - 1) as f64;
    let window = (per_sample * (sparkline::HISTORY_CAPACITY - 1) as f64).round() as u64;
    if window < 90 {
        format!("−{window} с")
    } else {
        format!("−{} мин", (window + 30) / 60)
    }
}
