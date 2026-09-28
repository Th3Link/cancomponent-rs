#![no_std]
#![no_main]

use cancomponents::button::Button;
use cancomponents::can;
use cancomponents::cli;
use cancomponents::config;
use cancomponents::config::config;
use cancomponents::console_log;
use cancomponents::device;
use cancomponents::echo_guard;
use cancomponents::extension::Extension;
use cancomponents::flash;
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
use esp_hal::gpio::Pin;
use esp_hal::timer::timg::TimerGroup;

esp_bootloader_esp_idf::esp_app_desc!();

#[esp_hal::main]
async fn main(spawner: Spawner) -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::_80MHz));

    let timg0 = TimerGroup::new(peripherals.TIMG0);
    esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);

    flash::init(peripherals.FLASH);
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

    // Commissioning console, up regardless of whether device_type/hwrev are
    // already configured. GPIO3/GPIO1 (ESP32 default UART0 pins) aren't used
    // by any device-type/hwrev pin mapping below.
    cli::init(
        peripherals.UART0,
        peripherals.GPIO3,
        peripherals.GPIO1,
        &spawner,
    )
    .await;

    let device_type = config()
        .await
        .get_u8(config::Key::DeviceType)
        .await
        .map(DeviceType::from);

    let hwrev = config().await.get_u8(config::Key::HardwareRevision).await;
    console_log!("device_type: {device_type:?}");
    console_log!("hwrev: {hwrev:?}");
    match (device_type, hwrev) {
        (Some(DeviceType::Relais), Some(2)) => {
            Relais::init(
                peripherals.I2C0,
                peripherals.GPIO21,
                peripherals.GPIO19,
                [0x26, 0x27],
                &spawner,
            )
            .await;
            Extension::init(
                peripherals.I2C1,
                peripherals.GPIO25.degrade(),
                peripherals.GPIO26.degrade(),
                peripherals.GPIO5.degrade(),
                peripherals.GPIO15.degrade(),
                None,
                &spawner,
            )
            .await;
        }
        (Some(DeviceType::Relais), Some(1)) => {
            Relais::init(
                peripherals.I2C0,
                peripherals.GPIO21,
                peripherals.GPIO19,
                [0x26, 0x27],
                &spawner,
            )
            .await;
            Extension::init(
                peripherals.I2C1,
                peripherals.GPIO15.degrade(),
                peripherals.GPIO16.degrade(),
                peripherals.GPIO17.degrade(),
                peripherals.GPIO18.degrade(),
                Some((peripherals.GPIO5.degrade(), peripherals.GPIO2.degrade())),
                &spawner,
            )
            .await;
        }

        (Some(DeviceType::Rollershutter), Some(1)) => {
            Relais::init(
                peripherals.I2C0,
                peripherals.GPIO21,
                peripherals.GPIO19,
                [0x26, 0x27],
                &spawner,
            )
            .await;
            Extension::init(
                peripherals.I2C1,
                peripherals.GPIO15.degrade(),
                peripherals.GPIO16.degrade(),
                peripherals.GPIO17.degrade(),
                peripherals.GPIO18.degrade(),
                Some((peripherals.GPIO5.degrade(), peripherals.GPIO2.degrade())),
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
            Extension::init(
                peripherals.I2C1,
                peripherals.GPIO4.degrade(),
                peripherals.GPIO16.degrade(),
                peripherals.GPIO17.degrade(),
                peripherals.GPIO18.degrade(),
                Some((peripherals.GPIO5.degrade(), peripherals.GPIO2.degrade())),
                &spawner,
            )
            .await;
        }
        (Some(DeviceType::Button), Some(2)) => {
            Button::init(
                peripherals.GPIO25,
                peripherals.GPIO26,
                peripherals.GPIO5,
                peripherals.GPIO15,
                &spawner,
            );
            Extension::init(
                peripherals.I2C1,
                peripherals.GPIO4.degrade(),
                peripherals.GPIO16.degrade(),
                peripherals.GPIO17.degrade(),
                peripherals.GPIO18.degrade(),
                None,
                &spawner,
            )
            .await;
        }

        (_, _) => {}
    };

    let data = [1];
    Timer::after(Duration::from_millis(5_000)).await;
    can::send_can_message(CanMessageType::Available, &data, false).await;
    Timer::after(Duration::from_millis(1_000)).await;
    can::send_can_message(CanMessageType::Available, &data, false).await;

    echo_guard::init(&spawner).await;

    loop {
        Timer::after(Duration::from_millis(3_000)).await;
    }
}
