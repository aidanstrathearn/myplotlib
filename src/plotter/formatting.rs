//! Number, tick, and hover label formatting.

use super::axes::AxisDomain;
use super::{AxisScale, YAxisSide};
use egui_plot::{GridMark, PlotPoint};

fn trim_decimal(mut value: String) -> String {
    if value.contains('.') {
        while value.ends_with('0') {
            value.pop();
        }
        if value.ends_with('.') {
            value.pop();
        }
    }
    value
}

pub(super) fn format_value(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }

    let absolute = value.abs();
    if !(0.001..10_000.0).contains(&absolute) {
        format!("{value:.3e}")
    } else {
        trim_decimal(format!("{value:.6}"))
    }
}

fn format_tick_value(value: f64, step_size: f64) -> String {
    if !value.is_finite() || !step_size.is_finite() || step_size <= 0.0 {
        return format_value(value);
    }

    let absolute = value.abs();
    if value != 0.0 && !(0.001..10_000.0).contains(&absolute) {
        let exponent = absolute.log10().floor();
        let scaled_step = step_size / 10.0_f64.powf(exponent);
        let decimals = (-scaled_step.log10()).ceil().clamp(0.0, 3.0) as usize;
        return format!("{value:.decimals$e}");
    }

    let decimals = (-step_size.log10()).ceil().clamp(0.0, 6.0) as usize;
    format!("{value:.decimals$}")
}

pub(super) fn format_log10_tick(mark: GridMark, _range: &std::ops::RangeInclusive<f64>) -> String {
    let mut exponent = mark.value.floor() as i32;
    let mut mantissa = 10.0_f64.powf(mark.value - f64::from(exponent)).round() as i32;

    // Floating-point error can put a decade tick just below its integer exponent.
    if mantissa == 10 {
        mantissa = 1;
        exponent += 1;
    }

    format!("{mantissa}e{exponent}")
}

pub(super) fn format_hover_label(
    name: &str,
    point: &PlotPoint,
    x_scale: AxisScale,
    y_scale: AxisScale,
) -> String {
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name}\n")
    };
    let x = format_value(x_scale.inverse(point.x));
    let y = format_value(y_scale.inverse(point.y));
    format!("{prefix}x = {x}\ny = {y}")
}

pub(super) fn format_mapped_tick(
    mark: GridMark,
    primary_domain: AxisDomain,
    axis_domain: AxisDomain,
    scale: AxisScale,
) -> String {
    let transformed = primary_domain.map_to(mark.value, axis_domain);
    let next_transformed = primary_domain.map_to(mark.value + mark.step_size, axis_domain);
    let value = scale.inverse(transformed);
    let next_value = scale.inverse(next_transformed);
    format_tick_value(value, (next_value - value).abs())
}

pub(super) fn format_twin_hover_label(
    name: &str,
    point: &PlotPoint,
    x_scale: AxisScale,
    axes: &[(String, YAxisSide, AxisScale, AxisDomain)],
    primary_domain: AxisDomain,
) -> String {
    let mut label = if name.is_empty() {
        String::new()
    } else {
        format!("{name}\n")
    };
    label.push_str(&format!("x = {}", format_value(x_scale.inverse(point.x))));

    for (axis_label, side, scale, domain) in axes {
        let transformed = primary_domain.map_to(point.y, *domain);
        let value = format_value(scale.inverse(transformed));
        let axis_label = if axis_label.is_empty() {
            match side {
                YAxisSide::Left => "y (left)",
                YAxisSide::Right => "y (right)",
            }
        } else {
            axis_label
        };
        label.push_str(&format!("\n{axis_label} = {value}"));
    }

    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mapped_ticks_round_to_their_effective_axis_step() {
        let mark = GridMark {
            value: 1.0,
            step_size: 1.0,
        };

        assert_eq!(
            format_mapped_tick(
                mark,
                AxisDomain::new(0.0, 3.0, "test"),
                AxisDomain::new(0.0, 1.0, "test"),
                AxisScale::Linear,
            ),
            "0.3"
        );
        assert_eq!(format_tick_value(100.242424, 0.5), "100.2");
    }

    #[test]
    fn log_ticks_and_hover_labels_use_original_values() {
        let broad_range = -1.0..=2.0;
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: -2.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e-2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 0.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e0"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0_f64.log10(),
                    step_size: 0.1,
                },
                &broad_range,
            ),
            "2e0"
        );

        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 2.0 + 4.0_f64.log10(),
                    step_size: 0.1,
                },
                &broad_range,
            ),
            "4e2"
        );
        assert_eq!(
            format_log10_tick(
                GridMark {
                    value: 3.0 - 1e-12,
                    step_size: 1.0,
                },
                &broad_range,
            ),
            "1e3"
        );

        let label = format_hover_label(
            "Signal",
            &PlotPoint::new(1.0, -2.0),
            AxisScale::Log10,
            AxisScale::Log10,
        );
        assert_eq!(label, "Signal\nx = 10\ny = 0.01");
    }
}
