use eframe::egui;
use std::sync::Arc;

#[derive(Clone, Debug)]
struct ScreenshotRequest {
    rect: egui::Rect,
    pixels_per_point: f32,
    filename: String,
    status_id: egui::Id,
}

#[derive(Clone)]
pub(crate) enum SaveStatus {
    Pending,
    Error(String),
}

pub(crate) fn status(ctx: &egui::Context, status_id: egui::Id) -> Option<SaveStatus> {
    ctx.data(|data| data.get_temp(status_id))
}

pub(crate) fn request(
    ctx: &egui::Context,
    rect: egui::Rect,
    filename: String,
    status_id: egui::Id,
) {
    ctx.data_mut(|data| data.insert_temp(status_id, SaveStatus::Pending));
    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(
        ScreenshotRequest {
            rect,
            pixels_per_point: ctx.pixels_per_point(),
            filename,
            status_id,
        },
    )));
}

pub(crate) fn handle_screenshot_events(ctx: &egui::Context) {
    let screenshots = ctx.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| {
                let egui::Event::Screenshot {
                    user_data, image, ..
                } = event
                else {
                    return None;
                };
                let request = user_data
                    .data
                    .as_ref()?
                    .downcast_ref::<ScreenshotRequest>()?;
                Some((request.clone(), Arc::clone(image)))
            })
            .collect::<Vec<_>>()
    });

    for (request, screenshot) in screenshots {
        let result = crop(&screenshot, request.rect, request.pixels_per_point)
            .and_then(|image| encode_png(&image))
            .and_then(|png| deliver_png(&png, &request.filename));

        ctx.data_mut(|data| match result {
            Ok(()) => {
                data.remove::<SaveStatus>(request.status_id);
            }
            Err(error) => {
                data.insert_temp(request.status_id, SaveStatus::Error(error));
            }
        });
        ctx.request_repaint();
    }
}

fn crop(
    screenshot: &egui::ColorImage,
    rect: egui::Rect,
    pixels_per_point: f32,
) -> Result<egui::ColorImage, String> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Err("the screenshot has an invalid display scale".to_owned());
    }

    let image_rect = egui::Rect::from_min_max(
        egui::Pos2::ZERO,
        egui::pos2(
            screenshot.width() as f32 / pixels_per_point,
            screenshot.height() as f32 / pixels_per_point,
        ),
    );
    let rect = rect.intersect(image_rect);
    if rect.width() < 1.0 / pixels_per_point || rect.height() < 1.0 / pixels_per_point {
        return Err("the plot is outside the captured image".to_owned());
    }

    Ok(screenshot.region(&rect, Some(pixels_per_point)))
}

fn encode_png(image: &egui::ColorImage) -> Result<Vec<u8>, String> {
    let width = u32::try_from(image.width())
        .map_err(|_| "the screenshot is too wide to encode as PNG".to_owned())?;
    let height = u32::try_from(image.height())
        .map_err(|_| "the screenshot is too tall to encode as PNG".to_owned())?;
    let mut bytes = Vec::new();

    {
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| format!("could not start PNG encoding: {error}"))?;
        writer
            .write_image_data(image.as_raw())
            .map_err(|error| format!("could not encode the PNG: {error}"))?;
    }

    Ok(bytes)
}

#[cfg(not(target_arch = "wasm32"))]
fn deliver_png(bytes: &[u8], filename: &str) -> Result<(), String> {
    let Some(path) = rfd::FileDialog::new()
        .add_filter("PNG image", &["png"])
        .set_file_name(filename)
        .save_file()
    else {
        return Ok(());
    };

    std::fs::write(&path, bytes)
        .map_err(|error| format!("could not save {}: {error}", path.display()))?;
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn deliver_png(bytes: &[u8], filename: &str) -> Result<(), String> {
    use wasm_bindgen::JsCast as _;

    let byte_array = js_sys::Uint8Array::from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&byte_array.buffer());

    let options = web_sys::BlobPropertyBag::new();
    options.set_type("image/png");
    let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options)
        .map_err(js_error)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(js_error)?;

    let result = (|| {
        let window =
            web_sys::window().ok_or_else(|| "the browser window is unavailable".to_owned())?;
        let document = window
            .document()
            .ok_or_else(|| "the browser document is unavailable".to_owned())?;
        let anchor = document
            .create_element("a")
            .map_err(js_error)?
            .dyn_into::<web_sys::HtmlAnchorElement>()
            .map_err(|element| js_error(element.into()))?;
        anchor.set_href(&url);
        anchor.set_download(filename);

        let body = document
            .body()
            .ok_or_else(|| "the browser document has no body".to_owned())?;
        body.append_child(&anchor).map_err(js_error)?;
        anchor.click();
        body.remove_child(&anchor).map_err(js_error)?;
        Ok(())
    })();

    let _ = web_sys::Url::revoke_object_url(&url);
    result
}

#[cfg(target_arch = "wasm32")]
fn js_error(error: wasm_bindgen::JsValue) -> String {
    error
        .as_string()
        .unwrap_or_else(|| format!("browser error: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop_uses_logical_coordinates_and_display_scale() {
        let image = egui::ColorImage::new(
            [8, 6],
            (0..48)
                .map(|index| egui::Color32::from_rgb(index, 0, 0))
                .collect(),
        );

        let cropped = crop(
            &image,
            egui::Rect::from_min_max(egui::pos2(1.0, 0.5), egui::pos2(3.0, 2.5)),
            2.0,
        )
        .unwrap();

        assert_eq!(cropped.size, [4, 4]);
        assert_eq!(cropped[(0, 0)], image[(2, 1)]);
        assert_eq!(cropped[(3, 3)], image[(5, 4)]);
    }

    #[test]
    fn png_encoding_preserves_image_dimensions() {
        let image = egui::ColorImage::filled([3, 2], egui::Color32::LIGHT_BLUE);
        let bytes = encode_png(&image).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let reader = decoder.read_info().unwrap();

        assert_eq!(reader.info().width, 3);
        assert_eq!(reader.info().height, 2);
        assert_eq!(reader.info().color_type, png::ColorType::Rgba);
    }
}
