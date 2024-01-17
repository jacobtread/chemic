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

    let mut default_input: Option<Device> = None;
    let mut default_output: Option<Device> = None;

    let is_default = args()
        .skip(1)
        // Convert to lowercase for case-insensitive matching
        .map(|arg| arg.to_lowercase())
        // Find a matching default arg
        .any(|arg| matches!(arg.as_str(), "default" | "--default" | "d" | "-d"));

    // Set the default input devices
    if is_default {
        default_input = host.default_input_device();
        default_output = host.default_output_device();
    }

    let input_device: NamedDevice = default_input
        // Create a named device
        .map(NamedDevice::from)
        // Prompt input device if none specified
        .unwrap_or_else(|| {
            prompt_device(&host, "Select input device to test", DeviceType::Input)
                .expect("Failed to select input device")
        });

    let output_device: NamedDevice = default_output
        // Create a named device
        .map(NamedDevice::from)
        // Prompt for an output device if none specified
        .unwrap_or_else(|| {
            prompt_device(&host, "Select output device to play to", DeviceType::Output)
                .expect("Failed to select output device")
        });

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

    // Build the streams
    let output_stream = match output.build_output_stream(
        output_config,
        create_converter_callback(conv),
        handle_error,
        None,
    ) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error while starting output stream: {}", err);
            panic!();
        }
    };

    let input_stream = match input.build_input_stream(
        input_config,
        create_producer_callback(producer),
        handle_error,
        None,
    ) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error while starting input stream: {}", err);
            panic!();
        }
    };

    // Play the streams
    if let Err(err) = output_stream.play() {
        eprintln!("Error while playing output stream: {}", err);
        panic!();
    };

    if let Err(err) = input_stream.play() {
        eprintln!("Error while playing input stream: {}", err);
        panic!();
    };

    println!("Playing microphone through output device...");
    println!("Press the ESCAPE or BACKSPACE key to stop..");

    // Wait for user input to stop the program
    loop {
        let key = Term::stderr().read_key()?;
        match key {
            // Stop capturing when a stop key is pressed
            Key::Escape | Key::Backspace | Key::Del | Key::CtrlC => break,
            // Another key was pushed
            _ => {}
        }
    }

    Ok(())
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

fn handle_error(error: StreamError) {
    eprint!("Error while streaming: {}", error);
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
    fn from(value: Device) -> Self {
        let device = value;
        let name = device
            .name()
            // Default "Unknown" name when name cannot be determined
            .unwrap_or_else(|_| "Unknown".to_string());
        Self { device, name }
    }
}

/// Type of a device
enum DeviceType {
    /// Input device
    Input,
    /// Ouput device
    Output,
}

/// Prompts the user for a input device using the provided `prompt`
/// the `output` option determines whether to include input or output
/// devices
fn prompt_device(host: &Host, prompt: &str, ty: DeviceType) -> io::Result<NamedDevice> {
    let theme = ColorfulTheme::default();
    let mut select = Select::with_theme(&theme);
    select.with_prompt(prompt);
    select.default(0);
    select.report(true);

    let mut devices: Vec<NamedDevice> = Vec::new();

    // Type bounds for the default device fn
    type DefaultDeviceFn = fn(&Host) -> Option<Device>;

    // Type alias for the filtered device iterator
    type DevicesFiltered = std::iter::Filter<Devices, fn(&Device) -> bool>;

    // Type bounds for the devices fn
    type DevicesFn = fn(&Host) -> Result<DevicesFiltered, DevicesError>;

    let (default_device, devices_fn): (DefaultDeviceFn, DevicesFn) = match ty {
        DeviceType::Input => (Host::default_input_device, Host::input_devices),
        DeviceType::Output => (Host::default_output_device, Host::output_devices),
    };

    // Add the default device
    devices.extend(default_device(host).map(NamedDevice::from_default));

    // Add all devices, includes the default device
    devices.extend(
        devices_fn(host)
            .expect("Unable to load devices")
            .map(NamedDevice::from),
    );

    // Add a select item for each device
    devices
        .iter()
        .for_each(|device| _ = select.item(&device.name));

    let index = select.interact()?;
    let device = devices.remove(index);

    Ok(device)
}
