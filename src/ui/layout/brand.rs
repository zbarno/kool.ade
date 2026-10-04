//! Embedded Kool.ad/e marks and decorative header art.
use egui::{Context, TextureHandle};

pub(crate) fn logo(ctx: &Context, compact: bool) -> TextureHandle {
    let id = egui::Id::new(if compact {
        "kool_ade_wordmark_texture"
    } else {
        "kool_ade_logo_texture"
    });
    if let Some(texture) = ctx.data_mut(|data| data.get_temp::<TextureHandle>(id)) {
        return texture;
    }
    let bytes: &[u8] = if compact {
        include_bytes!("../../../assets/brand/kool-ad-e-wordmark-transparent.png")
    } else {
        include_bytes!("../../../assets/brand/kool-ad-e-logo-transparent.png")
    };
    let texture = decode(
        ctx,
        bytes,
        if compact {
            "kool_ade_wordmark"
        } else {
            "kool_ade_logo"
        },
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

pub(crate) fn splash(ctx: &Context) -> TextureHandle {
    let id = egui::Id::new("kool_ade_punch_splash_texture");
    if let Some(texture) = ctx.data_mut(|data| data.get_temp::<TextureHandle>(id)) {
        return texture;
    }
    let texture = decode(
        ctx,
        include_bytes!("../../../assets/brand/tropical-punch-splash.png"),
        "kool_ade_punch_splash",
    );
    ctx.data_mut(|data| data.insert_temp(id, texture.clone()));
    texture
}

fn decode(ctx: &Context, bytes: &[u8], name: &str) -> TextureHandle {
    let limit = ctx.input(|input| input.max_texture_side).min(1024) as u32;
    let image = image::load_from_memory(bytes)
        .unwrap_or_else(|error| panic!("embedded {name} image is invalid: {error}"))
        .thumbnail(limit, limit)
        .to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color_image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR)
}
