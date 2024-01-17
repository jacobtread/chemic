use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Host, InputCallbackInfo, OutputCallbackInfo, Sample, StreamConfig, StreamError,
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

    let mut input_device: Option<Device> = None;
    let mut output_device: Option<Device> = None;

    // Search the program arguments for default options
    for arg in args().skip(1) {
        match arg.to_lowercase().as_str() {
            // Handle default device option
            "default" | "--default" | "d" | "-d" => {
                println!("Using default devices");

                input_device = host.default_input_device();
                output_device = host.default_output_device();
                break;
            }
            _ => {
                eprintln!("Unknown argument \"{}\"", arg);
            }
        }
    }

    // Prompt input device if none specified
    if input_device.is_none() {
        input_device = Some(prompt_input_device(&host)?)
    }

    // Prompt for an output device if none specified
    if output_device.is_none() {
        output_device = Some(prompt_output_device(&host)?)
    }

    let input_device = match input_device {
        Some(value) => value,
        None => {
            eprintln!("Missing input device");
            panic!();
        }
    };

    let output_device = match output_device {
        Some(value) => value,
        None => {
            eprintln!("Missing output device");
            panic!();
        }
    };

    let input_config: StreamConfig = input_device
        .default_input_config()
        .expect("No supported input configs")
        .into();

    let output_config: StreamConfig = output_device
        .default_output_config()
        .expect("No suppoorted output configs")
        .into();

    let input_name = input_device
        .name()
        .unwrap_or_else(|_| "Unknown".to_string());

    println!("== == == == Input Device == == == ==");
    println!("Name       : {}", input_name);
    println!("Channels   : {}", input_config.channels);
    println!("Sample Rate: {}Hz", input_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    let output_name = output_device
        .name()
        .unwrap_or_else(|_| "Unknown".to_string());

    println!("== == == == Output Device == == == ==");
    println!("Name       : {}", output_name);
    println!("Channels   : {}", output_config.channels);
    println!("Sample Rate: {}Hz", output_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    start_streams(input_device, &input_config, output_device, &output_config)
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

/// Prompt the user to choose their input device
fn prompt_input_device(host: &Host) -> io::Result<Device> {
    let theme = ColorfulTheme::default();
    let mut select = Select::with_theme(&theme);
    select.with_prompt("Select input device to test");
    select.default(0);
    select.report(true);
    let mut devices = Vec::new();

    // Append the default device
    if let Some(default) = host.default_input_device() {
        let name = if let Ok(name) = default.name() {
            format!("Default ({})", name)
        } else {
            "Default".to_string()
        };

        devices.push(default);
        select.item(name);
    }

    // Append all other known devices
    host.input_devices()
        .expect("Unable to load host input devices")
        .for_each(|device| {
            if let Ok(name) = device.name() {
                devices.push(device);
                select.item(name);
            }
        });

    let index = select.interact()?;
    let device = devices.remove(index);

    Ok(device)
}
/// Prompt the user to choose their input device
fn prompt_output_device(host: &Host) -> io::Result<Device> {
    let theme = ColorfulTheme::default();
    let mut select = Select::with_theme(&theme);
    select.with_prompt("Select output device to play to");
    select.default(0);
    let mut devices = Vec::new();

    // Append the default device
    if let Some(default) = host.default_output_device() {
        let name = if let Ok(name) = default.name() {
            format!("Default ({})", name)
        } else {
            "Default".to_string()
        };

        devices.push(default);
        select.item(name);
    }

    // Append all other known devices
    host.output_devices()
        .expect("Unable to load host output devices")
        .for_each(|device| {
            if let Ok(name) = device.name() {
                devices.push(device);
                select.item(name);
            }
        });

    let index = select.interact()?;
    let device = devices.remove(index);

    Ok(device)
}
