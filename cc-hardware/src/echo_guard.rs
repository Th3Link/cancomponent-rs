use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_time::{Duration, Timer};
use esp_println::println;

pub static ECHO_CHANNEL: Channel<CriticalSectionRawMutex, bool, 2> = Channel::new();

pub async fn init(spawner: &Spawner) {
    spawner.spawn(echo_guard_task()).unwrap();
}

#[embassy_executor::task]
pub async fn echo_guard_task() {
    println!("echo_guard_task started");
    let mut miss_count = 0;
    loop {
        let delay = Timer::after(Duration::from_secs(40));
        let echo_recv = ECHO_CHANNEL.receive();
        match select(echo_recv, delay).await {
            Either::First(_) => {
                miss_count = 0;
                Timer::after(Duration::from_secs(40)).await;
            }
            Either::Second(_) => {
                miss_count += 1;
                if miss_count > 3 {
                    println!("Echos missed, restarting");
                    Timer::after(Duration::from_secs(2)).await;
                    esp_hal::system::software_reset();
                }
            }
        }
    }
}
