use resvg::usvg;
use resvg::tiny_skia;
use std::fs::File;
use std::io::Read;

fn main() {
    println!("🧪 Testing SVG rendering with resvg...");

    // Test with the actual SVG file
    let svg_path = "/home/jenning/open-nexus-OS/recipes/gui/nexus-assets/source/ui/icons/apps/utilities-terminal.svg";

    // Read SVG file
    let mut file = match File::open(svg_path) {
        Ok(file) => file,
        Err(e) => {
            println!("❌ Failed to open SVG file: {}", e);
            return;
        }
    };

    let mut svg_content = String::new();
    if let Err(e) = file.read_to_string(&mut svg_content) {
        println!("❌ Failed to read SVG file: {}", e);
        return;
    }

    println!("📖 Read SVG file: {} bytes", svg_content.len());

    // Parse SVG
    let opt = usvg::Options::default();
    let tree = match usvg::Tree::from_str(&svg_content, &opt) {
        Ok(tree) => tree,
        Err(e) => {
            println!("❌ Failed to parse SVG: {}", e);
            return;
        }
    };

    println!("✅ SVG parsed successfully");
    println!("🔍 Original SVG size: {}x{}", tree.size().width(), tree.size().height());

    // Test different target sizes
    let test_sizes = [32, 48, 64, 96, 128];

    for target_size in &test_sizes {
        println!("\n🎯 Testing target size: {}x{}", target_size, target_size);

        // Create pixmap with exact target size
        let mut pixmap = match tiny_skia::Pixmap::new(*target_size, *target_size) {
            Some(pixmap) => pixmap,
            None => {
                println!("❌ Failed to create pixmap for size {}", target_size);
                continue;
            }
        };

        // Calculate scale to fit the SVG into the target size while preserving aspect ratio
        let svg_size = tree.size();
        let scale_x = *target_size as f32 / svg_size.width();
        let scale_y = *target_size as f32 / svg_size.height();
        let scale = scale_x.min(scale_y); // Preserve aspect ratio

        println!("🔍 Scale calculation: {}x{} -> {}x{} (scale: {})",
                 svg_size.width(), svg_size.height(), target_size, target_size, scale);

        // Create transform with uniform scaling to preserve aspect ratio
        let transform = tiny_skia::Transform::from_scale(scale, scale);

        // Render the SVG using the correct API
        resvg::render(&tree, transform, &mut pixmap.as_mut());

        println!("✅ Rendered successfully: {}x{}", pixmap.width(), pixmap.height());

        // Test with independent scaling (no aspect ratio preservation)
        let mut pixmap2 = match tiny_skia::Pixmap::new(*target_size, *target_size) {
            Some(pixmap) => pixmap,
            None => {
                println!("❌ Failed to create pixmap2 for size {}", target_size);
                continue;
            }
        };

        let transform2 = tiny_skia::Transform::from_scale(scale_x, scale_y);
        resvg::render(&tree, transform2, &mut pixmap2.as_mut());

        println!("✅ Rendered with independent scaling: {}x{}", pixmap2.width(), pixmap2.height());
    }

    println!("\n🎉 SVG rendering test completed!");
}
