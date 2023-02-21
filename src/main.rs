use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    Device, Host, StreamConfig, StreamError,
};
use dialoguer::{console::Term, theme::ColorfulTheme, Confirm, Select};
use std::{io, sync::mpsc};

fn main() -> io::Result<()> {
    println!(
        r#"
                                                           
______ __           _______ __          (=)    
|      |  |--.-----.|   |   |__|.----.  |x|   
|   ---|     |  -__||       |  ||  __|  | |  
|______|__|__|_____||__|_|__|__||____|  |_| 
                                        
CheMic - Microphone testing tool
"#
    );

    let host = cpal::default_host();

    let input_device = prompt_input_device(&host)?;
    let output_device = prompt_output_device(&host)?;
    let deep = prompt_deep_voice()?;

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

    start_streams(
        input_device,
        &input_config,
        output_device,
        &output_config,
        deep,
    )
}

pub fn start_streams(
    input: Device,
    ic: &StreamConfig,
    output: Device,
    oc: &StreamConfig,
    deep: bool,
) -> io::Result<()> {
    // Create conversion ratio
    let i_rate = ic.sample_rate;
    let o_rate = oc.sample_rate;

    let mut ratio = (1.0 / (i_rate.0 as f32 / o_rate.0 as f32)).ceil() as usize;

    println!("Conversion Ratio: 1 : {}", ratio);

    if deep {
        ratio += 1;
    }

    // Create the data sharing callbacks
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let data_out = move |out: &mut [f32], _: &cpal::OutputCallbackInfo| {
        let src = match rx.try_recv() {
            Ok(value) => value,
            Err(_) => return,
        };

        let mut src_index = 0;
        let mut out_index = 0;

        // Streching out the input data to match the sample rate of
        // the output data
        while src_index < src.len() && out_index + ratio < out.len() {
            for offset in 0..=ratio {
                out[out_index + offset] = src[src_index];
            }

            src_index += 1;
            out_index += ratio;
        }
    };

    let data_in = move |data: &[f32], _: &cpal::InputCallbackInfo| {
        tx.send(data.to_vec()).ok();
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

fn handle_error(error: StreamError) {
    eprint!("Error while streaming: {}", error);
}

fn prompt_deep_voice() -> io::Result<bool> {
    let theme = ColorfulTheme::default();
    Confirm::with_theme(&theme)
        .with_prompt("Enable deep voice?")
        .default(false)
        .show_default(true)
        .interact()
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
