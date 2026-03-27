#![no_std]
#![no_main]

use defmt::*;
use embassy_executor::Spawner;
use embassy_net::udp::{PacketMetadata, UdpSocket};
use embassy_net::{
    IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr, Stack, StackResources, StaticConfigV4,
};
use embassy_rp::clocks::RoscRng;
use embassy_rp::usb::{Driver, InterruptHandler};
use embassy_rp::{bind_interrupts, peripherals};
use embassy_usb::class::cdc_ncm::embassy_net::{Device, Runner, State as NetState};
use embassy_usb::class::cdc_ncm::{CdcNcmClass, State as CdcNcmState};
use embassy_usb::{Builder, Config, UsbDevice};
use edge_dhcp::{DhcpOption, Ipv4Addr, MessageType, Packet};
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

fn dhcp_message_type(packet: &Packet<'_>) -> Option<MessageType> {
    packet.options.iter().find_map(|opt| match opt {
        DhcpOption::MessageType(mt) => Some(mt),
        _ => None,
    })
}

#[embassy_executor::task]
async fn dhcp_server_task(stack: Stack<'static>) -> ! {
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx_buf = [0u8; 1536];
    let mut tx_buf = [0u8; 1536];
    let mut socket = UdpSocket::new(
        stack,
        &mut rx_meta,
        &mut rx_buf,
        &mut tx_meta,
        &mut tx_buf,
    );
    unwrap!(socket.bind(67));

    let server_ip = Ipv4Addr::new(172, 31, 1, 1);
    let offered_ip = Ipv4Addr::new(172, 31, 1, 2);
    let gateways = [server_ip];
    let subnet = Some(Ipv4Addr::new(255, 255, 255, 0));

    let mut in_buf = [0u8; 1536];
    let mut out_buf = [0u8; 1536];

    info!("DHCP server listening on UDP:67 (offer 172.31.1.2)");

    loop {
        let Ok((len, src)) = socket.recv_from(&mut in_buf).await else {
            warn!("dhcp recv error");
            continue;
        };

        let Ok(request) = Packet::decode(&in_buf[..len]) else {
            warn!("dhcp decode error");
            continue;
        };

        let Some(msg_type) = dhcp_message_type(&request) else {
            continue;
        };

        let reply_type = match msg_type {
            MessageType::Discover => MessageType::Offer,
            MessageType::Request => MessageType::Ack,
            MessageType::Release | MessageType::Decline | MessageType::Inform => continue,
            _ => continue,
        };

        // `reply()` が返す Options が `opt_buf` を借用するため、
        // ループを跨いで借用が残らないようにスコープを切ってエンコードまで完結させます。
        let mut opt_buf = [DhcpOption::Message(""); 16];
        let frame = {
            let reply_options = request.options.reply(
                reply_type,
                server_ip,
                7200,
                &gateways,
                subnet,
                &[],
                None,
                &mut opt_buf,
            );
            let reply = request.new_reply(Some(offered_ip), reply_options);
            match reply.encode(&mut out_buf) {
                Ok(f) => f,
                Err(_) => {
                    warn!("dhcp encode error");
                    continue;
                }
            }
        };

        let dst = if request.broadcast {
            IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::BROADCAST), 68)
        } else {
            IpEndpoint::new(IpAddress::Ipv4(Ipv4Address::new(172, 31, 1, 2)), 68)
        };

        if let Err(e) = socket.send_to(frame, dst).await {
            warn!("dhcp send error: {:?}", e);
            continue;
        }

        info!("DHCP {:?} -> {:?}, src={:?}", msg_type, reply_type, src);
    }
}

/// DHCP サンプル: CDC-NCM + Pico 側固定 IP + DHCPv4 サーバーで 172.31.1.2 を払い出します。
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
    let (stack, runner) = embassy_net::new(
        device,
        net_config,
        RESOURCES.init(StackResources::new()),
        seed,
    );
    spawner.spawn(net_task(runner).unwrap());
    spawner.spawn(dhcp_server_task(stack).unwrap());

    info!("CDC-NCM up. Pico IPv4 = 172.31.1.1/24");

    loop {
        embassy_time::Timer::after_secs(3600).await;
    }
}

