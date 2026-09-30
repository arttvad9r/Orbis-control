//! Telemetry history and the SVG line charts drawn from it.
//!
//! The production renderer (Slint software renderer) cannot rasterize `Path`
//! items, but it renders SVG images through resvg. Charts are therefore
//! emitted as small white-on-transparent SVG documents that the UI tints with
//! `colorize`: an opaque stroke plus a fading fill under the line.

use std::collections::VecDeque;

/// Number of samples kept per series (≈2 minutes at the default poll rate).
pub const HISTORY_CAPACITY: usize = 60;

/// Fixed-capacity series of observed integer samples. `None` marks a poll in
/// which the sensor reported nothing, so gaps are never drawn as real values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Series {
    samples: VecDeque<Option<i32>>,
}

impl Series {
    pub fn push(&mut self, value: Option<i32>) {
        if self.samples.len() == HISTORY_CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(value);
    }

    /// Most recent contiguous run of observed values (oldest first).
    pub fn recent_values(&self) -> Vec<i32> {
        let mut run: Vec<i32> = self
            .samples
            .iter()
            .rev()
            .map_while(|sample| *sample)
            .collect();
        run.reverse();
        run
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }
}

/// Vertical scaling of a chart.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Range {
    /// Fit the observed values with some headroom (at least `min_span` tall),
    /// so small fluctuations stay visible in a compact sparkline.
    Auto { min_span: f32 },
    /// Fixed axis shared with labels drawn by the UI.
    Fixed { min: f32, max: f32 },
}

/// Chart canvas in SVG user units. Slint rasterizes an SVG at the target size
/// but keeps the document's aspect ratio (then stretches the bitmap without
/// filtering), so the UI boxes use exactly these proportions:
/// sparklines 5:2 and charts 11:5.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Canvas {
    pub width: f32,
    pub height: f32,
    pub stroke: f32,
    /// Draw a dot on the newest sample.
    pub head_dot: bool,
}

pub const SPARK: Canvas = Canvas {
    width: 160.0,
    height: 64.0,
    stroke: 2.4,
    head_dot: false,
};

pub const CHART: Canvas = Canvas {
    width: 550.0,
    height: 250.0,
    stroke: 3.0,
    head_dot: true,
};

/// Build the SVG document for `values`, or `None` when fewer than two samples
/// exist (a single point is not a trend).
pub fn line_chart_svg(values: &[i32], range: Range, canvas: Canvas) -> Option<String> {
    if values.len() < 2 {
        return None;
    }
    // Sensors report whole units; a light moving average keeps the trend
    // readable instead of drawing one-degree stair steps.
    let values = smooth(values);
    let values = values.as_slice();
    let (lo, hi) = match range {
        Range::Fixed { min, max } => (min, max.max(min + 1.0)),
        Range::Auto { min_span } => {
            let min = values.iter().copied().fold(f32::INFINITY, f32::min);
            let max = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            let span = (max - min).max(min_span);
            let mid = (max + min) / 2.0;
            // Headroom keeps the line off the top edge.
            (mid - span * 0.6, mid + span * 0.6)
        }
    };
    let edge = canvas.stroke + if canvas.head_dot { 4.0 } else { 1.0 };
    // A fixed axis must line up with the UI's grid, so it gets no inset.
    let pad = match range {
        Range::Fixed { .. } => 0.0,
        Range::Auto { .. } => edge,
    };
    let usable_h = canvas.height - 2.0 * pad;
    let usable_w = canvas.width - if canvas.head_dot { edge } else { 0.0 };
    // Plot the series against the full history width so a young series grows
    // in from the right instead of stretching over the whole chart.
    let slots = HISTORY_CAPACITY.max(values.len()) - 1;
    let step = usable_w / slots as f32;
    let first_x = usable_w - step * (values.len() - 1) as f32;
    let points: Vec<(f32, f32)> = values
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let t = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
            (first_x + step * i as f32, pad + usable_h * (1.0 - t))
        })
        .collect();

    let line = smooth_path(&points);
    let (last_x, last_y) = points[points.len() - 1];
    let (first_px, _) = points[0];
    let bottom = canvas.height;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" preserveAspectRatio=\"none\">\
<defs><linearGradient id=\"f\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\">\
<stop offset=\"0\" stop-color=\"#fff\" stop-opacity=\"0.34\"/>\
<stop offset=\"1\" stop-color=\"#fff\" stop-opacity=\"0\"/></linearGradient></defs>\
<path d=\"{line} L{last_x:.1} {bottom} L{first_px:.1} {bottom} Z\" fill=\"url(#f)\"/>\
<path d=\"{line}\" fill=\"none\" stroke=\"#fff\" stroke-width=\"{s}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
        w = canvas.width,
        h = canvas.height,
        s = canvas.stroke,
    );
    if canvas.head_dot {
        svg.push_str(&format!(
            "<circle cx=\"{last_x:.1}\" cy=\"{last_y:.1}\" r=\"{r}\" fill=\"#fff\"/>",
            r = canvas.stroke + 1.5
        ));
    }
    svg.push_str("</svg>");
    Some(svg)
}

/// Centered moving average over five samples (narrower at the edges, so the
/// newest point still reflects the latest reading closely).
fn smooth(values: &[i32]) -> Vec<f32> {
    let n = values.len();
    (0..n)
        .map(|i| {
            let reach = 2.min(i).min(n - 1 - i);
            let window = &values[i - reach..=i + reach];
            window.iter().sum::<i32>() as f32 / window.len() as f32
        })
        .collect()
}

/// Catmull-Rom spline through `points`, expressed as cubic Bézier segments.
fn smooth_path(points: &[(f32, f32)]) -> String {
    let mut d = format!("M{:.1} {:.1}", points[0].0, points[0].1);
    for i in 0..points.len() - 1 {
        let p0 = points[i.saturating_sub(1)];
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = points[(i + 2).min(points.len() - 1)];
        let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
        let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
        d.push_str(&format!(
            " C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
            c1.0, c1.1, c2.0, c2.1, p2.0, p2.1
        ));
    }
    d
}

/// Leading integer of a formatted reading such as `"2300 rpm"` or `"23 W"`.
pub fn leading_number(text: &str) -> Option<i32> {
    let digits: String = text
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn series_keeps_capacity_and_latest_run() {
        let mut series = Series::default();
        for v in 0..(HISTORY_CAPACITY as i32 + 5) {
            series.push(Some(v));
        }
        assert_eq!(series.len(), HISTORY_CAPACITY);
        assert_eq!(series.recent_values().first(), Some(&5));

        series.push(None);
        series.push(Some(7));
        series.push(Some(9));
        assert_eq!(series.recent_values(), vec![7, 9]);
    }

    #[test]
    fn chart_needs_two_points() {
        assert!(line_chart_svg(&[42], Range::Auto { min_span: 5.0 }, SPARK).is_none());
        let svg = line_chart_svg(
            &[40, 45, 50],
            Range::Fixed {
                min: 0.0,
                max: 100.0,
            },
            CHART,
        )
        .expect("svg");
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn chart_svg_is_loadable() {
        let svg = line_chart_svg(&[1, 5, 3, 8], Range::Auto { min_span: 2.0 }, SPARK).unwrap();
        let image = slint::Image::load_from_svg_data(svg.as_bytes()).expect("valid svg");
        assert!(image.size().width > 0);
    }

    #[test]
    fn leading_number_parses_formatted_readings() {
        assert_eq!(leading_number("2300 rpm"), Some(2300));
        assert_eq!(leading_number("23 W"), Some(23));
        assert_eq!(leading_number("—"), None);
    }
}
