#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::{Ipv4Address, Ipv4Cidr, StackResources, StaticConfigV4};
use embassy_rp::clocks::RoscRng;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{bind_interrupts, peripherals};
use embassy_usb::class::cdc_ncm::embassy_net::{Device, Runner, State as NetState};
use embassy_usb::class::cdc_ncm::{CdcNcmClass, State as CdcNcmState};
use embassy_usb::{Builder, Config, UsbDevice};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    USBCTRL_IRQ => InterruptHandler<peripherals::USB>;
});

type MyDriver = Driver<'static, peripherals::USB>;

const MTU: usize = 1514;

#[embassy_executor::task]
async fn usb_task(mut device: UsbDevice<'static, MyDriver>) -> ! {
    device.run().await
}

#[embassy_executor::task]
async fn usb_ncm_task(class: Runner<'static, MyDriver, MTU>) -> ! {
    class.run().await
}

#[embassy_executor::task]
async fn net_task(mut runner: embassy_net::Runner<'static, Device<'static, MTU>>) -> ! {
    runner.run().await
}

/// 最小サンプル: CDC-NCM として認識させ、固定 IPv4 のスタックを起動します。
#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let p = embassy_rp::init(Default::default());
    let mut rng = RoscRng;

    let driver = Driver::new(p.USB, Irqs);

    let mut config = Config::new(0xc0de, 0xcafe);
    config.manufacturer = Some("pico-usb-cdc-ncm-rust");
    config.product = Some("Pico USB CDC-NCM");
    config.serial_number = Some("00000001");
    config.max_power = 100;
    config.max_packet_size_0 = 64;

    static CONFIG_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static BOS_DESC: StaticCell<[u8; 256]> = StaticCell::new();
    static CONTROL_BUF: StaticCell<[u8; 128]> = StaticCell::new();

    let mut builder = Builder::new(
        driver,
        config,
        &mut CONFIG_DESC.init([0; 256])[..],
        &mut BOS_DESC.init([0; 256])[..],
        &mut [], // no msos descriptors
        &mut CONTROL_BUF.init([0; 128])[..],
    );

    let our_mac_addr = [0xCC, 0xCC, 0xCC, 0xCC, 0xCC, 0xCC];
    let host_mac_addr = [0x88, 0x88, 0x88, 0x88, 0x88, 0x88];

    static CDC_NCM_STATE: StaticCell<CdcNcmState> = StaticCell::new();
    let class = CdcNcmClass::new(
        &mut builder,
        CDC_NCM_STATE.init(CdcNcmState::new()),
        host_mac_addr,
        64,
    );

    let usb = builder.build();
    spawner.spawn(usb_task(usb).unwrap());

    static NET_STATE: StaticCell<NetState<MTU, 4, 4>> = StaticCell::new();
    let (runner, device) =
        class.into_embassy_net_device(NET_STATE.init(NetState::new()), our_mac_addr);
    spawner.spawn(usb_ncm_task(runner).unwrap());

    let ip = Ipv4Address::new(172, 31, 1, 1);
    let net_config = embassy_net::Config::ipv4_static(StaticConfigV4 {
        address: Ipv4Cidr::new(ip, 24),
        gateway: None,
        dns_servers: heapless::Vec::new(),
    });

    let seed = rng.next_u64();
    static RESOURCES: StaticCell<StackResources<4>> = StaticCell::new();
    let (_stack, runner) = embassy_net::new(
        device,
        net_config,
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());

    info!("CDC-NCM up. Pico IPv4 = 172.31.1.1/24");

    loop {
        embassy_time::Timer::after_secs(3600).await;
    }
}

