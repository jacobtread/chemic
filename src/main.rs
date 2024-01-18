use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Devices, DevicesError, Host, InputCallbackInfo, OutputCallbackInfo, Sample,
    StreamConfig, StreamError,
};
use dasp_interpolate::linear::Linear;
use dasp_signal::{interpolate::Converter, Signal};
use dialoguer::{
    console::{Key, Term},
    theme::ColorfulTheme,
    Select,
};
use ringbuf::{HeapConsumer, HeapProducer, HeapRb};
use std::{env::args, io};

fn main() -> io::Result<()> {
    println!(
        r#"
                                                           
 ______ __           _______ __         (=)    
|      |  |--.-----.|   |   |__|.----.  |x|   
|   ---|     |  -__||       |  ||  __|  | |  
|______|__|__|_____||__|_|__|__||____|  |_| 
                                        
CheMic - Microphone testing tool
"#
    );

    let host = cpal::default_host();

    let mut input_device: Option<NamedDevice> = None;
    let mut output_device: Option<NamedDevice> = None;

    // Check for the default mode arg
    let is_default = args()
        .skip(1)
        // Convert to lowercase for case-insensitive matching
        .map(|arg| arg.to_lowercase())
        // Find a matching default arg
        .any(|arg| matches!(arg.as_str(), "default" | "--default" | "d" | "-d"));

    // Set the default input devices
    if is_default {
        input_device = host
            .default_input_device()
            // Create a named device
            .map(NamedDevice::from_default);
        output_device = host
            .default_output_device()
            // Create a named device
            .map(NamedDevice::from_default);
    }

    // Create the named devices and prompt for them if they are missing
    let input_device: NamedDevice = input_device
        // Prompt input device if none specified
        .unwrap_or_else(|| {
            prompt_device(&host, "Select input device to test", DeviceType::Input)
                .expect("Failed to select input device")
        });

    let output_device: NamedDevice = output_device
        // Prompt for an output device if none specified
        .unwrap_or_else(|| {
            prompt_device(&host, "Select output device to play to", DeviceType::Output)
                .expect("Failed to select output device")
        });

    // Obtain the device configuration
    let input_config: StreamConfig = input_device
        .device
        .default_input_config()
        .expect("No supported input configs")
        .into();

    let output_config: StreamConfig = output_device
        .device
        .default_output_config()
        .expect("No suppoorted output configs")
        .into();

    // Print the device information
    println!("== == == == Input Device == == == ==");
    println!("Name       : {}", input_device.name);
    println!("Channels   : {}", input_config.channels);
    println!("Sample Rate: {}Hz", input_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    println!("== == == == Output Device == == == ==");
    println!("Name       : {}", output_device.name);
    println!("Channels   : {}", output_config.channels);
    println!("Sample Rate: {}Hz", output_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    start_streams(
        input_device.device,
        &input_config,
        output_device.device,
        &output_config,
    )
}

/// Create a input stream callback that pushes the callback data onto
/// the provided `producer`
fn create_producer_callback(
    mut producer: HeapProducer<f32>,
) -> impl FnMut(&[f32], &InputCallbackInfo) {
    move |data, _| {
        // Write the data to the producer
        producer.push_slice(data);
    }
}

/// Creates an output stream callback that stores the output from the
/// provided `converter` onto the callback output buffer
fn create_converter_callback(
    mut converter: Converter<ConsumerSignal, Linear<f32>>,
) -> impl FnMut(&mut [f32], &OutputCallbackInfo) {
    move |data, _| {
        // Fill the output data with the values from the converter
        data.fill_with(|| converter.next());
    }
}

fn start_streams(
    input: Device,
    input_config: &StreamConfig,
    output: Device,
    output_config: &StreamConfig,
) -> io::Result<()> {
    // Create the ring buffer for the input data
    let ring: HeapRb<f32> = HeapRb::new(input_config.sample_rate.0 as usize * 2);
    let (producer, consumer) = ring.split();

    // Wrap the consumer for use as a signal
    let source = ConsumerSignal(consumer);

    // We need to interpolate to the target sample rate
    let conv = Converter::from_hz_to_hz(
        source,
        Linear::new(0f32, 0f32),
        input_config.sample_rate.0 as f64,
        output_config.sample_rate.0 as f64,
    );

    // Small closure for handling stream errors
    let handle_error = |error: StreamError| eprint!("Error while streaming: {}", error);

    // Build the streams
    let output_stream = output
        .build_output_stream(
            output_config,
            create_converter_callback(conv),
            handle_error,
            None,
        )
        .map_err(io::Error::other)?;

    let input_stream = input
        .build_input_stream(
            input_config,
            create_producer_callback(producer),
            handle_error,
            None,
        )
        .map_err(io::Error::other)?;

    // Play the streams
    output_stream.play().map_err(io::Error::other)?;
    input_stream.play().map_err(io::Error::other)?;

    println!("Playing microphone through output device...");
    println!("Press the ESCAPE or BACKSPACE key to stop..");

    // Wait for the stop key
    while !stop_key_pressed() {}

    Ok(())
}

/// Reads a input from the terminal, returns whether the
/// provided input matches a stop key
fn stop_key_pressed() -> bool {
    let key = Term::stderr().read_key().expect("Failed to read stop key");
    matches!(key, Key::Escape | Key::Backspace | Key::Del | Key::CtrlC)
}

/// [Signal] implementation for producing frames from a [HeapConsumer]
/// allowing it to be used as a signal to convert values from
/// the consumer between Hz values.
///
/// Will produce silence when the consumer has no values to produce
struct ConsumerSignal(HeapConsumer<f32>);

impl Signal for ConsumerSignal {
    type Frame = f32;

    fn next(&mut self) -> Self::Frame {
        self.0
            .pop()
            // Use silence if no more values are available
            .unwrap_or(Sample::EQUILIBRIUM)
    }
}

/// [Device] with an additional name that has already been
/// determined, might be a generic name like "Default" or "Unknown"
struct NamedDevice {
    /// The device itself
    device: Device,
    /// The name of the device
    name: String,
}

impl NamedDevice {
    /// Creates a new named device from the provided device, wraps
    /// the device name with "Default" to indicate its a default
    /// device
    fn from_default(device: Device) -> Self {
        let mut device = NamedDevice::from(device);
        device.name = format!("Default ({})", device.name);
        device
    }
}

impl From<Device> for NamedDevice {
    fn from(device: Device) -> Self {
        let name = device
            .name()
            // Default "Unknown" name when name cannot be determined
            .unwrap_or_else(|_| "Unknown".to_string());
        Self { device, name }
    }
}

/// Type of a [Device]
#[derive(Clone, Copy)]
enum DeviceType {
    /// Input device
    Input,
    /// Ouput device
    Output,
}

/// Finds the default device for the provided `ty` on the `host`
/// will return [None] if it was unable to find one
fn get_default_device(host: &Host, ty: DeviceType) -> Option<NamedDevice> {
    // Type bounds for the default device fn
    type DefaultDeviceFn = fn(&Host) -> Option<Device>;

    let default_device: DefaultDeviceFn = match ty {
        DeviceType::Input => Host::default_input_device,
        DeviceType::Output => Host::default_output_device,
    };

    default_device(host).map(NamedDevice::from_default)
}

/// Finds all devices that match the provided `ty` on the `host`
/// includes a duplicate of the default device
fn get_devices(host: &Host, ty: DeviceType) -> Vec<NamedDevice> {
    // Type alias for the filtered device iterator
    type DevicesFiltered = std::iter::Filter<Devices, fn(&Device) -> bool>;
    // Type bounds for the devices fn
    type DevicesFn = fn(&Host) -> Result<DevicesFiltered, DevicesError>;

    // Determine the function for getting the devices of the provided type
    let devices_fn: DevicesFn = match ty {
        DeviceType::Input => Host::input_devices,
        DeviceType::Output => Host::output_devices,
    };

    // Include the default device as the first device
    get_default_device(host, ty)
        .into_iter()
        // Include all other devices (Duplicate of default device)
        .chain(
            devices_fn(host)
                .expect("Unable to load devices")
                .map(NamedDevice::from),
        )
        .collect()
}

/// Prompts the user for a device using the provided `prompt` shows
/// only devices matching the provided `ty` on the `host`
fn prompt_device(host: &Host, prompt: &str, ty: DeviceType) -> io::Result<NamedDevice> {
    // Get all available devices
    let mut devices: Vec<NamedDevice> = get_devices(host, ty);

    // Handle no devices
    if devices.is_empty() {
        return Err(io::Error::other("No devices available"));
    }

    // Collec the device names
    let device_names: Vec<&str> = devices.iter().map(|device| device.name.as_str()).collect();

    // Create the selection prompt
    let theme = ColorfulTheme::default();
    let index = Select::with_theme(&theme)
        .with_prompt(prompt)
        .default(0)
        .report(true)
        .items(&device_names)
        .interact()
        .map_err(io::Error::other)?;
    let device = devices.remove(index);

    Ok(device)
}
