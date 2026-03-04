// src/pretty.rs
use serde_json::Value;

fn get_str<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("-")
}

pub fn print_profile(profile: &Value) {
    println!(
        "\nWelcome, {}! Authentication successful.",
        get_str(profile, "user_name")
    );
    println!("\nYour Account Details are as follows:");
    println!("User ID: {}", get_str(profile, "user_id"));
    println!("User Type: {}", get_str(profile, "user_type"));
    println!("Email ID: {}", get_str(profile, "email"));
    println!("User Short Name: {}", get_str(profile, "user_shortname"));
    println!("Broker: {}", get_str(profile, "broker"));

    println!(
        "Exchanges: {}",
        profile.get("exchanges").unwrap_or(&Value::Null)
    );
    println!(
        "Products: {}",
        profile.get("products").unwrap_or(&Value::Null)
    );
    println!(
        "Order Types: {}",
        profile.get("order_types").unwrap_or(&Value::Null)
    );
}
