use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Host, Sample, StreamConfig, StreamError,
};
use dasp_interpolate::linear::Linear;
use dasp_signal::{interpolate::Converter, Signal};
use dialoguer::{console::Term, theme::ColorfulTheme, Select};
use ringbuf::{Consumer, HeapRb, SharedRb};
use std::{io, mem::MaybeUninit, sync::Arc};

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

    let input_device = prompt_input_device(&host)?;
    let output_device = prompt_output_device(&host)?;

    let input_config: StreamConfig = input_device
        .default_input_config()
        .expect("No supported input configs")
        .into();

    let output_config: StreamConfig = output_device
        .default_output_config()
        .expect("No suppoorted output configs")
        .into();

    let input_name = match input_device.name() {
        Ok(value) => value,
        Err(_) => "Unknown".to_string(),
    };

    println!("== == == == Input Device == == == ==");
    println!("Name       : {}", input_name);
    println!("Channels   : {}", input_config.channels);
    println!("Sample Rate: {}Hz", input_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    let output_name = match output_device.name() {
        Ok(value) => value,
        Err(_) => "Unknown".to_string(),
    };

    println!("== == == == Output Device == == == ==");
    println!("Name       : {}", output_name);
    println!("Channels   : {}", output_config.channels);
    println!("Sample Rate: {}Hz", output_config.sample_rate.0);
    println!("== == == == == === === == == == == ==\n\n");

    start_streams(input_device, &input_config, output_device, &output_config)
}

pub fn start_streams(
    input: Device,
    ic: &StreamConfig,
    output: Device,
    oc: &StreamConfig,
) -> io::Result<()> {
    // The buffer to share samples
    let ring = HeapRb::<f32>::new(ic.sample_rate.0 as usize * 2);
    let (mut producer, consumer) = ring.split();

    // Consumer source
    let source = ConsumerSignal(consumer);

    // We need to interpolate to the target sample rate
    let mut conv = Converter::from_hz_to_hz(
        source,
        Linear::new(0f32, 0f32),
        ic.sample_rate.0 as f64,
        oc.sample_rate.0 as f64,
    )
    .until_exhausted();

    let data_out = move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
        for index in 0..out.len() {
            let value = match conv.next() {
                Some(value) => value,
                None => break,
            };

            out[index] = value;
        }
    };

    let data_in = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        for &value in data {
            let _ = producer.push(value);
        }
    };

    // Build the streams
    let output_stream = match output.build_output_stream(&oc, data_out, handle_error, None) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error while starting output stream: {}", err);
            panic!();
        }
    };

    let input_stream = match input.build_input_stream(&ic, data_in, handle_error, None) {
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
    println!("Press any key to stop..");

    // Wait for user input to stop the program
    Term::stderr().read_key()?;

    Ok(())
}

/// Wrapper over a consumer to allow it to be used as a signal
/// for conversion between Hz values
pub struct ConsumerSignal(Consumer<f32, Arc<SharedRb<f32, Vec<MaybeUninit<f32>>>>>);

impl Signal for ConsumerSignal {
    type Frame = f32;

    fn next(&mut self) -> Self::Frame {
        match self.0.pop() {
            Some(value) => value,
            None => Sample::EQUILIBRIUM,
        }
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
