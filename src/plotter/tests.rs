use super::*;

#[test]
fn y_offset_shifts_all_curves_without_mutating_or_clamping_the_data() {
    let mut plot = Plotter::new();
    let first = plot.add_points(vec![[0.0, 0.0], [1.0, 1.0]]).id();
    plot.add_points(vec![[0.0, 0.2], [1.0, 0.8]]).points_only();
    plot.axhline(0.5);
    for offset in [0.25, -0.25, 0.0] {
        plot.y_offset(offset);
        let rendered = plot.transform_for_rendering();
        assert_eq!(
            rendered.series_segments[0],
            vec![vec![[0.0, offset], [1.0, 1.0 + offset]]]
        );
        assert_eq!(
            rendered.series_segments[1],
            vec![vec![[0.0, 0.2 + offset], [1.0, 0.8 + offset]]]
        );
        assert_eq!(rendered.horizontal_lines, vec![0.5]);
        assert_eq!(plot.series[first.0].points, vec![[0.0, 0.0], [1.0, 1.0]]);
    }
}

#[test]
fn y_gain_scales_curves_and_error_bands_before_adding_the_offset() {
    let mut plot = Plotter::new();
    let mean = plot.add_points(vec![[0.0, 0.25], [1.0, 0.75]]).id();
    plot.add_points(vec![[0.0, 0.125], [1.0, 0.625]]);
    plot.add_points(vec![[0.0, 0.375], [1.0, 0.875]])
        .points_only();
    plot.axhline(0.5);
    plot.y_gain(2.0);
    plot.y_offset(-0.25);

    let rendered = plot.transform_for_rendering();
    assert_eq!(
        rendered.series_segments,
        vec![
            vec![vec![[0.0, 0.25], [1.0, 1.25]]],
            vec![vec![[0.0, 0.0], [1.0, 1.0]]],
            vec![vec![[0.0, 0.5], [1.0, 1.5]]],
        ]
    );
    assert_eq!(rendered.horizontal_lines, vec![0.5]);
    assert_eq!(plot.series[mean.0].points, vec![[0.0, 0.25], [1.0, 0.75]]);

    plot.y_gain(0.0);
    plot.y_offset(0.5);
    assert!(
        plot.transform_for_rendering()
            .series_segments
            .iter()
            .all(|segments| segments == &vec![vec![[0.0, 0.5], [1.0, 0.5]]])
    );
}

#[test]
fn y_calibration_applies_to_future_streamed_data_without_accumulating() {
    let mut plot = Plotter::new();
    let series = plot.add_points(Vec::new()).id();
    plot.y_gain(2.0);
    plot.y_offset(0.25);
    plot.replace_points(series, vec![[0.0, 0.5]]);
    plot.append_points(series, [[1.0, 0.75]]);
    assert_eq!(
        plot.transform_for_rendering().series_segments[0],
        vec![vec![[0.0, 1.25], [1.0, 1.75]]]
    );
    plot.y_gain(0.5);
    plot.y_offset(-0.25);
    plot.replace_points(series, vec![[2.0, 0.5]]);
    assert_eq!(
        plot.transform_for_rendering().series_segments[0],
        vec![vec![[2.0, 0.0]]]
    );
    assert_eq!(plot.series[series.0].points, vec![[2.0, 0.5]]);
}

#[test]
fn summary_updates_preserve_order_and_add_missing_labels() {
    let mut plot = Plotter::new();
    plot.add_summary("Variance", "Waiting");
    plot.add_summary("Duration", "1 ms");
    plot.set_summary("Variance", "2e-6");
    plot.set_summary("RMS", "1.414e-3");
    assert_eq!(plot.summary.len(), 3);
    assert_eq!(plot.summary[0].label, "Variance");
    assert_eq!(plot.summary[0].value, "2e-6");
    assert_eq!(plot.summary[1].label, "Duration");
    assert_eq!(plot.summary[1].value, "1 ms");
    assert_eq!(plot.summary[2].label, "RMS");
    assert_eq!(plot.summary[2].value, "1.414e-3");
}

#[test]
fn series_ids_update_existing_data_without_changing_its_identity() {
    let mut plot = Plotter::new();
    let first = plot.add_points(vec![[0.0, 1.0]]).id();
    let second = plot.add_points(vec![[10.0, 11.0]]).id();

    assert_ne!(first, second);

    plot.append_points(first, [[1.0, 2.0], [2.0, 3.0]]);
    assert_eq!(
        plot.series[first.0].points,
        vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]]
    );
    assert_eq!(plot.series[second.0].points, vec![[10.0, 11.0]]);

    plot.replace_points(first, vec![[4.0, 5.0]]);
    assert_eq!(plot.series[first.0].points, vec![[4.0, 5.0]]);

    plot.clear_points(first);
    assert!(plot.series[first.0].points.is_empty());
}

#[test]
fn series_can_be_rendered_as_points_without_changing_its_identity() {
    let mut plot = Plotter::new();
    let series = plot.add_points(vec![[1.0, 2.0]]).points_only().id();

    assert_eq!(plot.series[series.0].style, SeriesStyle::Points);

    plot.replace_points(series, vec![[3.0, 4.0]]);

    assert_eq!(plot.series[series.0].style, SeriesStyle::Points);
    assert_eq!(plot.series[series.0].points, vec![[3.0, 4.0]]);
}
