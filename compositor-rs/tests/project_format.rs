//! The `.comp` interchange checks: the format the macOS build reads and writes.
//!
//! Two directions matter. A project written here has to satisfy every rule in
//! `docs/writing-comp-files.md` (bare image file names, bottom-to-top layer order, the exact
//! blend-mode spellings). A project written by hand the way that document describes has to open
//! here with the same pixels and metadata.

use std::path::Path;
use std::sync::Arc;

use rustphotoshop::core::blend::BlendMode;
use rustphotoshop::core::document::{Document, Guide, LayerKind, Mask, Sampling, Transform};
use rustphotoshop::core::pixel::{GrayImage, PremulImage};
use rustphotoshop::io::{image_io, project};

/// The minimal manifest from the format documentation, with a real PNG beside it.
fn write_hand_written_project(root: &Path) -> String {
    let images = root.join("images");
    std::fs::create_dir_all(&images).expect("images directory");
    let layer_id = "6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F";
    let pixels = PremulImage::filled(4, 4, [255, 0, 0, 255]);
    image_io::save_png(&pixels, &images.join(format!("{layer_id}.png"))).expect("layer png");
    let manifest = format!(
        r#"{{
  "format": "com.compositor.project",
  "version": 11,
  "colorSpace": "sRGB",
  "documentID": "0C5E7A91-3B2D-4F6A-8E1C-9D0B7A6F5E4D",
  "width": 4,
  "height": 4,
  "resolution": 72,
  "activeLayerID": "{layer_id}",
  "layers": [
    {{
      "id": "{layer_id}",
      "name": "Background",
      "imageFile": "{layer_id}.png",
      "isVisible": true,
      "isGroup": false,
      "opacity": 1,
      "blendMode": "Normal",
      "transform": {{
        "origin": [0, 0],
        "size": [4, 4],
        "rotation": 0,
        "flipX": false,
        "flipY": false,
        "sampling": "High quality"
      }}
    }}
  ]
}}"#
    );
    std::fs::write(root.join("manifest.json"), manifest).expect("manifest");
    layer_id.to_owned()
}

#[test]
fn a_hand_written_project_opens_with_its_pixels() {
    let directory = tempfile::tempdir().expect("temp dir");
    let root = directory.path().join("Hand.comp");
    std::fs::create_dir_all(&root).expect("package");
    let layer_id = write_hand_written_project(&root);

    assert!(project::is_project(&root));
    let document = project::load(&root).expect("load");
    assert_eq!((document.width, document.height), (4, 4));
    assert_eq!(document.layers.len(), 1);
    assert_eq!(document.layers[0].name, "Background");
    assert_eq!(
        document.layers[0].id.to_string().to_uppercase(),
        layer_id,
        "图层 ID 必须原样保留"
    );
    let image = document.layers[0].image.clone().expect("pixels");
    assert_eq!(image.pixel(0, 0), Some([255, 0, 0, 255]));
}

#[test]
fn a_project_written_here_follows_the_documented_rules() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("Rules.comp");
    let mut document = Document::new(8, 6);
    document.add_layer(
        "底层",
        Arc::new(PremulImage::filled(8, 6, [10, 20, 30, 255])),
        None,
    );
    let top = document.add_layer(
        "顶层",
        Arc::new(PremulImage::filled(4, 4, [200, 100, 50, 255])),
        None,
    );
    {
        let layer = document.layer_mut(top).expect("layer");
        layer.blend = BlendMode::LinearDodge;
        layer.opacity = 0.5;
        layer.mask = Some(Mask {
            image: Arc::new(GrayImage::new(4, 4, 255)),
            linked: true,
            placement: None,
        });
    }
    project::save(&mut document, &path).expect("save");

    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("manifest.json")).expect("read"))
            .expect("json");
    assert_eq!(manifest["format"], "com.compositor.project");
    assert_eq!(manifest["version"], 11);
    assert_eq!(manifest["colorSpace"], "sRGB");
    assert!(manifest["documentID"].is_string());
    let layers = manifest["layers"].as_array().expect("layers");
    assert_eq!(layers.len(), 2);
    // Bottom to top, so the last layer draws on top.
    assert_eq!(layers[0]["name"], "底层");
    assert_eq!(layers[1]["name"], "顶层");
    for layer in layers {
        let id = layer["id"].as_str().expect("id");
        assert_eq!(id, id.to_uppercase(), "ID 必须大写");
        let image_file = layer["imageFile"].as_str().expect("imageFile");
        assert!(!image_file.contains('/'), "imageFile 只能是文件名");
        assert_eq!(image_file, format!("{id}.png"));
        assert!(path.join("images").join(image_file).is_file());
        assert!(layer["transform"]["origin"].is_array());
        assert!(layer["transform"]["size"].is_array());
    }
    // The exact spelling from the format's blend-mode list.
    assert_eq!(layers[1]["blendMode"], "Linear Dodge (Add)");
    assert_eq!(layers[1]["opacity"], 0.5);
    let top_id = layers[1]["id"].as_str().expect("id");
    assert_eq!(layers[1]["maskFile"], format!("{top_id}.mask.png"));
    assert_eq!(layers[1]["maskEnabled"], true);
    // Finder's Space-bar preview, which the macOS build writes too.
    assert!(path.join("QuickLook/Preview.jpg").is_file());
}

#[test]
fn groups_masks_and_adjustment_layers_survive_a_round_trip() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("结构.comp");
    let mut document = Document::new(16, 16);
    let base = document.add_layer(
        "底图",
        Arc::new(PremulImage::filled(16, 16, [40, 90, 160, 255])),
        None,
    );
    let patch = document.add_layer(
        "补丁",
        Arc::new(PremulImage::filled(8, 8, [220, 60, 40, 255])),
        None,
    );
    {
        let layer = document.layer_mut(patch).expect("layer");
        layer.transform = Transform {
            origin: [4.0, 4.0],
            size: [8.0, 8.0],
            rotation: 15.0,
            flip_x: true,
            flip_y: false,
            sampling: Sampling::Smooth,
        };
        layer.mask = Some(Mask {
            image: Arc::new(GrayImage::new(8, 8, 128)),
            linked: false,
            placement: Some(Transform::full(16, 16)),
        });
    }
    let folder = document.group_layers(&[patch], "文件夹").expect("group");
    let adjustment = rustphotoshop::core::ops::add_adjustment_layer(
        &mut document,
        rustphotoshop::core::adjustment::Adjustment::HueSaturation {
            hue: 30.0,
            saturation: -20.0,
            lightness: 5.0,
            colorize: false,
        },
    );
    document.guides.push(Guide {
        id: uuid::Uuid::new_v4(),
        horizontal: false,
        position: 7.5,
    });
    document.active_layer = Some(base);
    let before = rustphotoshop::core::composite::composite_document(&document);

    project::save(&mut document, &path).expect("save");
    let loaded = project::load(&path).expect("load");

    assert!(loaded.is_group(folder), "文件夹结构丢失");
    assert_eq!(loaded.layer(patch).expect("patch").parent, Some(folder));
    assert_eq!(
        loaded.layer(adjustment).expect("adjustment").kind,
        LayerKind::Adjustment
    );
    let loaded_patch = loaded.layer(patch).expect("patch");
    assert_eq!(loaded_patch.transform.rotation, 15.0);
    assert!(loaded_patch.transform.flip_x);
    let loaded_mask = loaded_patch.mask.as_ref().expect("mask");
    assert_eq!(loaded_mask.image.pixel(0, 0), 128);
    assert!(!loaded_mask.linked, "未链接的蒙版状态丢失");
    assert!(loaded_mask.placement.is_some(), "蒙版自身的位置丢失");
    assert_eq!(loaded.guides.len(), 1, "参考线丢失");
    match loaded
        .layer(adjustment)
        .expect("adjustment")
        .adjustment
        .as_ref()
    {
        Some(rustphotoshop::core::adjustment::Adjustment::HueSaturation { hue, .. }) => {
            assert!((hue - 30.0).abs() < 1e-9, "调整参数丢失");
        }
        other => panic!("调整图层类型丢失：{other:?}"),
    }
    // The picture has to come out the same: the format round trip is lossless.
    let after = rustphotoshop::core::composite::composite_document(&loaded);
    assert_eq!(before.as_slice(), after.as_slice(), "往返后画面发生变化");
}

#[test]
fn a_broken_project_is_refused_with_a_readable_reason() {
    let directory = tempfile::tempdir().expect("temp dir");
    let path = directory.path().join("Broken.comp");
    std::fs::create_dir_all(&path).expect("package");
    let manifest = r#"{
      "format": "com.compositor.project",
      "version": 11,
      "colorSpace": "sRGB",
      "documentID": "0C5E7A91-3B2D-4F6A-8E1C-9D0B7A6F5E4D",
      "width": 4,
      "height": 4,
      "layers": [{
        "id": "6F1D3C2A-0B7E-4E8A-9C4D-2A1B3C4D5E6F",
        "name": "Missing",
        "isVisible": true,
        "imageFile": "../../../etc/passwd",
        "transform": { "origin": [0, 0], "size": [4, 4] }
      }]
    }"#;
    std::fs::write(path.join("manifest.json"), manifest).expect("manifest");
    let error = project::load(&path).expect_err("load must fail");
    let message = error.to_string();
    assert!(
        message.contains("缺失") || message.contains("损坏"),
        "错误信息不够清楚：{message}"
    );
}

#[test]
fn a_layer_can_leave_as_an_image_file() {
    // A project is a package, but any picture inside it can be exported on its own.
    let directory = tempfile::tempdir().expect("temp dir");
    let mut document = Document::new(6, 6);
    document.add_layer(
        "图层",
        Arc::new(PremulImage::filled(6, 6, [12, 200, 90, 255])),
        None,
    );
    let layer_image = document.layers[0].image.clone().expect("pixels");
    let png = directory.path().join("layer.png");
    image_io::save_png(&layer_image, &png).expect("save png");
    assert_eq!(image_io::load(&png).expect("load png"), *layer_image);
    // And the composite can leave as a JPEG, which has no alpha channel.
    let flattened = rustphotoshop::core::composite::composite_document(&document);
    let jpeg = directory.path().join("flattened.jpg");
    image_io::save_jpeg(&flattened, &jpeg, 92, [255, 255, 255, 255]).expect("save jpeg");
    assert!(image_io::load(&jpeg).is_ok());
}
