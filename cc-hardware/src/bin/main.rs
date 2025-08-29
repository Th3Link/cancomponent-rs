#![no_std]
#![no_main]

use cancomponents::button::Button;
use cancomponents::can;
use cancomponents::config;
use cancomponents::config::config;
use cancomponents::device;
use cancomponents::echo_guard;
use cancomponents::gpio_interrupt;
use cancomponents::relais::Relais;
use cancomponents::update;
use cancomponents_core::can_message_type::CanMessageType;
use cancomponents_core::device_type::DeviceType;
use embassy_executor::Spawner;
use embassy_time::Duration;
use embassy_time::Timer;
use esp_backtrace as _;
use esp_hal::clock::CpuClock;
use esp_hal::timer::timg::TimerGroup;
use esp_hal_embassy::main;

/*struct ExtensionGpio {
    pin0: Pin + 'static,
    pin1: Pin + 'static,
    pin2: Pin + 'static,
    pin3: Pin + 'static,
}*/

#[main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz));

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_hal_embassy::init(timg0.timer0);

    config::init().await;
    device::init().await;
    update::init(&spawner).await;
    gpio_interrupt::init(peripherals.IO_MUX);
    can::init(
        peripherals.TWAI0,
        peripherals.GPIO14,
        peripherals.GPIO13,
        &spawner,
    )
    .await;

    let device_type = config()
        .await
        .get_u8(config::Key::DeviceType)
        .await
        .and_then(|v| DeviceType::try_from(v).ok());

    let hwrev = config().await.get_u8(config::Key::HardwareRevision).await;

    let _extension_gpios = match (device_type, hwrev) {
        (Some(DeviceType::Relais), Some(_)) => {
            Relais::init(
                peripherals.I2C0,
                peripherals.GPIO21,
                peripherals.GPIO19,
                [0x26, 0x27],
                &spawner,
            )
            .await;
        }
        (Some(DeviceType::Button), Some(1)) => {
            Button::init(
                peripherals.GPIO33,
                peripherals.GPIO35,
                peripherals.GPIO12,
                peripherals.GPIO34,
                &spawner,
            );
        }
        (Some(DeviceType::Button), Some(2)) => {
            Button::init(
                peripherals.GPIO25,
                peripherals.GPIO26,
                peripherals.GPIO5,
                peripherals.GPIO15,
                &spawner,
            );
        }

        (_, _) => {}
    };

    let data = [1];
    Timer::after(Duration::from_millis(5_000)).await;
    can::send_can_message(CanMessageType::Available, &data, false).await;
    Timer::after(Duration::from_millis(1_000)).await;
    can::send_can_message(CanMessageType::Available, &data, false).await;

    echo_guard::init(&spawner).await;
    /*
        if let Some(extension) = config()
            .await
            .get_u8(config::Key::ExtensionMode)
            .await
            .and_then(|v| Extension::try_from(v).ok())
        {
            match extension {
                Extension::Relais => {}
                Extension::Button => Button::init(
                    peripherals.GPIO25,
                    peripherals.GPIO26,
                    peripherals.GPIO5,
                    peripherals.GPIO15,
                    &spawner,
                ),
                Extension::Pwm => {}
                Extension::Sensors => {}
            }
        }
    */
    loop {
        Timer::after(Duration::from_millis(3_000)).await;
    }
}
