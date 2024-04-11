use std::time::Duration;

use crabgrab::prelude::*;
use crabgrab::feature::content_picker::{pick_sharable_content, SharableContentPickerConfig};

#[tokio::main]
async fn main() {
    let config = SharableContentPickerConfig {
        display: true,
        window: true,
        excluded_apps: vec![]
    };
    let sharable_content = pick_sharable_content(config).await;
    println!("sharable content: {}", sharable_content.is_ok()); 
}