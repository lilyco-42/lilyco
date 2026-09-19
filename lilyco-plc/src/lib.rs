//! # Lilyco PLC — Token 2 Anything 的工业机器桥
//!
//! 零依赖 Modbus TCP 客户端（自研 MBAP 帧实现，见 `modbus`）+
//! 派生应用（`PlcRead` T0 只读 / `PlcWrite` T2 能力令牌）。
//!
//! 设计要点：
//! - **零依赖**：MBAP/TCP 帧只有几十行，不值得为此引入 tokio-modbus 全家桶
//!   （lyco 信条 5：最小化验证，原子化构建）
//! - **天生受门控**：`PlcWrite` 声明 `safety = "t2"`，注册进 [`lilyco_core::registry::Registry`]
//!   即被 P0 安全门包住——自动化面默认拒绝，能力令牌策略放行
//! - **自带遥测**：读取/写入都通过 `ctx.telemetry` 上报数据点，
//!   Agent 在 MCP progress 通道实时看到寄存器值（P1 遥测流）
//! - **Mock PLC**：[`modbus::MockPlc`] 进程内模拟寄存器空间，测试与演示无需真硬件
//!
//! ```ignore
//! let mut client = ModbusTcp::connect("127.0.0.1:5020", 1)?;
//! let regs = client.read_holding_registers(0, 4)?;   // fc 0x03
//! client.write_single_register(0, 42)?;              // fc 0x06
//! ```

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::time::Duration;

/// 单次收发的读写超时
const IO_TIMEOUT: Duration = Duration::from_secs(3);

/// Modbus 通讯错误
#[derive(Debug)]
pub enum PlcError {
    /// 底层 IO 错误（连接 / 超时）
    Io(std::io::Error),
    /// 帧格式 / 协议状态错误
    Protocol(String),
    /// PLC 返回的 Modbus 异常响应（function | 0x80 + 异常码）
    Exception {
        /// 原功能码
        function: u8,
        /// Modbus 异常码（0x01 非法功能、0x02 非法地址、0x03 非法数量…）
        code: u8,
    },
}

impl std::fmt::Display for PlcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlcError::Io(e) => write!(f, "IO 错误: {e}"),
            PlcError::Protocol(m) => write!(f, "协议错误: {m}"),
            PlcError::Exception { function, code } => {
                write!(f, "PLC 异常响应: fc={function:#04x} code={code:#04x}")
            }
        }
    }
}

impl std::error::Error for PlcError {}

impl From<std::io::Error> for PlcError {
    fn from(e: std::io::Error) -> Self {
        PlcError::Io(e)
    }
}

/// 零依赖 Modbus TCP 客户端（MBAP 帧，同步阻塞 + 超时）
///
/// 支持 fc 0x03 读保持寄存器、0x06 写单寄存器、0x05 写单线圈。
/// 单连接串行事务（Modbus TCP 天然一问一答，无需流水线）。
pub struct ModbusTcp {
    stream: TcpStream,
    unit_id: u8,
    tid: u16,
}

impl ModbusTcp {
    /// 连接 PLC（`addr` 形如 `127.0.0.1:5020`），`unit_id` 一般为 1
    pub fn connect(addr: impl ToSocketAddrs, unit_id: u8) -> Result<Self, PlcError> {
        let stream = TcpStream::connect(addr)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        Ok(Self {
            stream,
            unit_id,
            tid: 0,
        })
    }

    /// 发一帧事务：MBAP 头 + PDU，读回响应 PDU（异常响应转 [`PlcError::Exception`]）
    fn transact(&mut self, pdu: &[u8]) -> Result<Vec<u8>, PlcError> {
        self.tid = self.tid.wrapping_add(1);
        let tid = self.tid;
        let len = (pdu.len() + 1) as u16; // unit id + pdu

        let mut frame = Vec::with_capacity(7 + pdu.len());
        frame.extend_from_slice(&tid.to_be_bytes());
        frame.extend_from_slice(&0u16.to_be_bytes()); // protocol id，恒 0
        frame.extend_from_slice(&len.to_be_bytes());
        frame.push(self.unit_id);
        frame.extend_from_slice(pdu);
        self.stream.write_all(&frame)?;

        // 响应 MBAP 头 7 字节
        let mut head = [0u8; 7];
        read_exact(&mut self.stream, &mut head)?;
        let rlen = u16::from_be_bytes([head[4], head[5]]) as usize;
        if rlen < 2 || rlen > 254 {
            return Err(PlcError::Protocol(format!("非法 MBAP 长度 {rlen}")));
        }
        if head[6] != self.unit_id {
            return Err(PlcError::Protocol(format!(
                "unit id 不匹配: 期望 {} 收到 {}",
                self.unit_id, head[6]
            )));
        }
        let mut resp = vec![0u8; rlen - 1];
        read_exact(&mut self.stream, &mut resp)?;

        if resp[0] & 0x80 != 0 {
            return Err(PlcError::Exception {
                function: resp[0] & 0x7F,
                code: resp.get(1).copied().unwrap_or(0),
            });
        }
        Ok(resp)
    }

    /// fc 0x03：读保持寄存器（`count` 1..=125）
    pub fn read_holding_registers(&mut self, addr: u16, count: u16) -> Result<Vec<u16>, PlcError> {
        if count == 0 || count > 125 {
            return Err(PlcError::Protocol("count 需在 1..=125".into()));
        }
        let pdu = [
            0x03,
            addr.to_be_bytes()[0],
            addr.to_be_bytes()[1],
            count.to_be_bytes()[0],
            count.to_be_bytes()[1],
        ];
        let resp = self.transact(&pdu)?;
        if resp[0] != 0x03 {
            return Err(PlcError::Protocol(format!(
                "期望 fc=0x03，收到 {:#04x}",
                resp[0]
            )));
        }
        let bc = resp[1] as usize;
        if bc != count as usize * 2 || bc + 2 != resp.len() {
            return Err(PlcError::Protocol(format!("字节计数不匹配: bc={bc}")));
        }
        Ok(resp[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect())
    }

    /// fc 0x06：写单个保持寄存器（响应回显请求）
    pub fn write_single_register(&mut self, addr: u16, value: u16) -> Result<(), PlcError> {
        let pdu = [
            0x06,
            addr.to_be_bytes()[0],
            addr.to_be_bytes()[1],
            value.to_be_bytes()[0],
            value.to_be_bytes()[1],
        ];
        let resp = self.transact(&pdu)?;
        if resp[0] != 0x06 || resp.len() != 5 || resp[1..] != pdu[1..] {
            return Err(PlcError::Protocol("写单寄存器回显不匹配".into()));
        }
        Ok(())
    }

    /// fc 0x05：写单线圈（0xFF00 = ON，0x0000 = OFF）
    pub fn write_single_coil(&mut self, addr: u16, on: bool) -> Result<(), PlcError> {
        let value: u16 = if on { 0xFF00 } else { 0x0000 };
        let pdu = [
            0x05,
            addr.to_be_bytes()[0],
            addr.to_be_bytes()[1],
            value.to_be_bytes()[0],
            value.to_be_bytes()[1],
        ];
        let resp = self.transact(&pdu)?;
        if resp[0] != 0x05 || resp.len() != 5 || resp[1..] != pdu[1..] {
            return Err(PlcError::Protocol("写线圈回显不匹配".into()));
        }
        Ok(())
    }
}

/// 精确读满 `buf.len()` 字节；对端关闭即协议错误
fn read_exact(stream: &mut TcpStream, buf: &mut [u8]) -> Result<(), PlcError> {
    let mut off = 0;
    while off < buf.len() {
        let n = stream.read(&mut buf[off..])?;
        if n == 0 {
            return Err(PlcError::Protocol("连接已关闭".into()));
        }
        off += n;
    }
    Ok(())
}

// ── Mock PLC（测试 / 演示，无需真硬件） ────────────────────

/// 进程内模拟 PLC：单线程模拟保持寄存器空间，支持 fc 0x03 / 0x06 / 0x05
///
/// ```ignore
/// let plc = MockPlc::spawn(vec![10, 20, 30])?;   // 三个初始寄存器
/// let mut c = ModbusTcp::connect(plc.addr, 1)?;
/// assert_eq!(c.read_holding_registers(0, 3)?, vec![10, 20, 30]);
/// ```
pub struct MockPlc {
    /// 监听地址（127.0.0.1 随机端口）
    pub addr: SocketAddr,
}

impl MockPlc {
    /// 启动 mock PLC（后台线程，accept 单连接多事务）
    pub fn spawn(initial: Vec<u16>) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        std::thread::spawn(move || {
            // 循环 accept：每个测试一个连接，多测试各自独立
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                mock_conn(&mut s, &initial);
            }
        });
        Ok(Self { addr })
    }
}

/// 单连接事务循环（寄存器空间每连接独立初始化）
fn mock_conn(stream: &mut TcpStream, initial: &[u16]) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let mut regs = initial.to_vec();
    loop {
        let mut head = [0u8; 7];
        if read_exact(stream, &mut head).is_err() {
            return;
        }
        let rlen = u16::from_be_bytes([head[4], head[5]]) as usize;
        if rlen < 2 {
            return;
        }
        let mut pdu = vec![0u8; rlen - 1];
        if read_exact(stream, &mut pdu).is_err() {
            return;
        }
        let resp: Vec<u8> = match pdu[0] {
            0x03 => {
                // 读保持寄存器
                if pdu.len() != 5 {
                    exception(0x03, 0x01)
                } else {
                    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
                    let count = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
                    if count == 0 || count > 125 || addr + count > regs.len() {
                        exception(0x03, 0x02)
                    } else {
                        let mut out = vec![0x03, (count * 2) as u8];
                        for r in &regs[addr..addr + count] {
                            out.extend_from_slice(&r.to_be_bytes());
                        }
                        out
                    }
                }
            }
            0x06 => {
                // 写单寄存器：落盘并回显
                if pdu.len() != 5 {
                    exception(0x06, 0x01)
                } else {
                    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
                    let value = u16::from_be_bytes([pdu[3], pdu[4]]);
                    if addr >= regs.len() {
                        exception(0x06, 0x02)
                    } else {
                        regs[addr] = value;
                        pdu.clone()
                    }
                }
            }
            0x05 => {
                // 写单线圈：0xFF00=1 / 0x0000=0，落盘到寄存器位并回显
                if pdu.len() != 5 {
                    exception(0x05, 0x01)
                } else {
                    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
                    let raw = u16::from_be_bytes([pdu[3], pdu[4]]);
                    if raw != 0x0000 && raw != 0xFF00 {
                        exception(0x05, 0x03)
                    } else if addr >= regs.len() {
                        exception(0x05, 0x02)
                    } else {
                        regs[addr] = u16::from(raw == 0xFF00);
                        pdu.clone()
                    }
                }
            }
            fc => exception(fc, 0x01), // 非法功能
        };
        // MBAP 响应：tid/proto/unit 回显头，len = unit + resp
        let mut frame = vec![head[0], head[1], 0, 0];
        frame.extend_from_slice(&((resp.len() + 1) as u16).to_be_bytes());
        frame.push(head[6]);
        frame.extend_from_slice(&resp);
        if stream.write_all(&frame).is_err() {
            return;
        }
    }
}

/// 构造 Modbus 异常响应 PDU
fn exception(function: u8, code: u8) -> Vec<u8> {
    vec![function | 0x80, code]
}

pub mod app;

pub use app::{PlcRead, PlcWrite};
