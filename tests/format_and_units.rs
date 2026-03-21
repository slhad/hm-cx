// Simple unit/format tests for mapping formatting helpers
#[test]
fn format_sensor_value_examples() {
    // Ensure formatting matches expected asset-like strings
    let t = hm_cx::mapping::format_sensor_value(42.0, "Temperature");
    assert_eq!(t, "42.00 °C");

    let v = hm_cx::mapping::format_sensor_value(4.011, "Voltage");
    assert_eq!(v, "4.011 V");

    let f = hm_cx::mapping::format_sensor_value(1164.0, "Fan");
    assert_eq!(f, "1164 RPM");
}
