/// NetEm Wrapper
/// NetEm - Network Emulator
///
/// NetEm is an enhancement of the Linux traffic control facilities that
/// allow to add delay, packet loss, duplication and more other
/// characteristics to packets outgoing from a selected network
/// interface. NetEm is built using the existing Quality Of Service (QOS)
/// and Differentiated Services (diffserv) facilities in the Linux
/// kernel.
use crate::error::Exception;
use regex::Regex;
use serde_derive::{Deserialize, Serialize};
use std::str::FromStr;
use tokio_net::process::Command;

type Percentage = f64;
type Millisecond = f64;

trait ToPercentageString {
    fn to_pct_string(&self) -> String;
}

impl ToPercentageString for Percentage {
    fn to_pct_string(&self) -> String {
        format!("{}%", self)
    }
}

trait ToMillisecondString {
    fn to_ms_string(&self) -> String;
}

impl ToMillisecondString for Millisecond {
    fn to_ms_string(&self) -> String {
        format!("{}ms", self)
    }
}

/// refer to: http://man7.org/linux/man-pages/man8/tc-netem.8.html
/// tc qdisc ... dev DEVICE ] add netem OPTIONS
///
///       OPTIONS := [ LIMIT ] [ DELAY ] [ LOSS ] [ CORRUPT ] [ DUPLICATION ] [
///       REORDERING ] [ RATE ] [ SLOT ]
trait Control {
    fn to_args(&self) -> Vec<String>;
}

/// LIMIT := limit packets
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Limit {
    packets: i32,
}

impl Control for Limit {
    fn to_args(&self) -> Vec<String> {
        vec!["limit".into(), format!("{}", self.packets)]
    }
}

lazy_static! {
    static ref LIMIT_REGEX: Regex = Regex::new(r"limit\s(?P<packets>[-\d]+)").unwrap();
}

impl FromStr for Limit {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = LIMIT_REGEX.captures(s) {
            let packets: i32 = captures
                .name("packets")
                .ok_or("get limit packets error")?
                .as_str()
                .parse()?;

            Ok(Limit { packets })
        } else {
            Err("no limit".into())
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone, PartialEq, Eq)]
enum Distribution {
    #[serde(rename = "uniform")]
    Uniform,
    #[serde(rename = "normal")]
    Normal,
    #[serde(rename = "pareto")]
    Pareto,
    #[serde(rename = "paretonormal")]
    ParetoNormal,
}

impl From<Distribution> for String {
    fn from(distribution: Distribution) -> Self {
        match distribution {
            Distribution::Uniform => "uniform",
            Distribution::Normal => "normal",
            Distribution::Pareto => "pareto",
            Distribution::ParetoNormal => "paretonormal",
        }
        .to_string()
    }
}

/// DELAY := delay TIME [ JITTER [ CORRELATION ]]]
///        [ distribution { uniform | normal | pareto |  paretonormal } ]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Delay {
    time: Millisecond,
    #[serde(skip_serializing_if = "Option::is_none")]
    jitter: Option<Millisecond>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distribution: Option<Distribution>,
}

impl Control for Delay {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(3);

        v.push("delay".into());
        v.push(self.time.to_ms_string());

        if let Some(jitter) = self.jitter {
            v.push(jitter.to_ms_string());
            if let Some(correlation) = self.correlation {
                v.push(correlation.to_pct_string());
            }
        }

        if let Some(distribution) = self.distribution {
            v.push("distribution".into());
            v.push(distribution.into());
        }

        v
    }
}

lazy_static! {
    static ref DELAY_REGEX: Regex = Regex::new(
        r"delay\s(?P<time>[\d\.]+)ms(\s{2}(?P<jitter>[\d\.]+)ms\s((?P<correlation>[\d\.]+)%)?)?"
    )
    .unwrap();
}

impl FromStr for Delay {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = DELAY_REGEX.captures(s) {
            let time: Millisecond = captures
                .name("time")
                .ok_or("get delay time error")?
                .as_str()
                .parse()?;

            let jitter: Option<Millisecond> = match captures.name("jitter") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            let correlation: Option<Percentage> = if jitter.is_some() {
                match captures.name("correlation") {
                    Some(s) => s.as_str().parse().ok(),
                    None => None,
                }
            } else {
                None
            };

            // TODO: distribution

            Ok(Delay {
                time,
                jitter,
                correlation,
                distribution: None,
            })
        } else {
            Err("no delay".into())
        }
    }
}

/// LOSS := loss { random PERCENT [ CORRELATION ]  |
///                state p13 [ p31 [ p32 [ p23 [ p14]]]] |
///                gemodel p [ r [ 1-h [ 1-k ]]] }  [ ecn ]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Loss {
    percent: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,

    // TODO: | state | gemodel
    ecn: bool,
}

impl Control for Loss {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);
        v.push("loss".into());

        v.push("random".into());
        v.push(self.percent.to_pct_string());

        if let Some(random) = self.correlation {
            v.push(random.to_pct_string());
        }

        if self.ecn {
            v.push("ecn".into());
        }

        v
    }
}

lazy_static! {
    static ref LOSS_REGEX: Regex =
        Regex::new(r"loss\s(?P<percent>[\d\.]+)%(\s(?P<correlation>[\d\.]+)%)?(.*\s(?P<ecn>ecn))?")
            .unwrap();
}

impl FromStr for Loss {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = LOSS_REGEX.captures(s) {
            let percent: Percentage = captures
                .name("percent")
                .ok_or("get loss percent error")?
                .as_str()
                .parse()?;

            let correlation: Option<Percentage> = match captures.name("correlation") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            let ecn = captures.name("ecn").is_some();

            Ok(Loss {
                percent,
                correlation,
                ecn,
            })
        } else {
            Err("no loss".into())
        }
    }
}

/// CORRUPT := corrupt PERCENT [ CORRELATION ]]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Corrupt {
    percent: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
}

impl Control for Corrupt {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("corrupt".into());
        v.push(self.percent.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        v
    }
}

lazy_static! {
    static ref CORRUPT_REGEX: Regex =
        Regex::new(r"corrupt\s(?P<percent>[\d\.]+)%(\s(?P<correlation>[\d\.]+)%)?").unwrap();
}

impl FromStr for Corrupt {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = CORRUPT_REGEX.captures(s) {
            let percent: Percentage = captures
                .name("percent")
                .ok_or("get corrupt percent error")?
                .as_str()
                .parse()?;

            let correlation: Option<Percentage> = match captures.name("correlation") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            Ok(Corrupt {
                percent,
                correlation,
            })
        } else {
            Err("no corrupt".into())
        }
    }
}

/// DUPLICATION := duplicate PERCENT [ CORRELATION ]]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Duplicate {
    percent: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
}

impl Control for Duplicate {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("duplicate".into());
        v.push(self.percent.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        v
    }
}

lazy_static! {
    static ref DUPLICATE_REGEX: Regex =
        Regex::new(r"duplicate\s(?P<percent>[\d\.]+)%(\s(?P<correlation>[\d\.]+)%)?").unwrap();
}

impl FromStr for Duplicate {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = DUPLICATE_REGEX.captures(s) {
            let percent: Percentage = captures
                .name("percent")
                .ok_or("get duplicate percent error")?
                .as_str()
                .parse()?;

            let correlation: Option<Percentage> = match captures.name("correlation") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            Ok(Duplicate {
                percent,
                correlation,
            })
        } else {
            Err("no duplicate".into())
        }
    }
}

/// REORDERING := reorder PERCENT [ CORRELATION ] [ gap DISTANCE ]
#[derive(Serialize, Deserialize, Debug, PartialEq)]
struct Reorder {
    percent: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<u32>,
}

impl Control for Reorder {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("reorder".into());
        v.push(self.percent.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        if let Some(distance) = self.distance {
            v.push("gap".to_owned());
            v.push(distance.to_string())
        }

        v
    }
}

lazy_static! {
    static ref REORDER_REGEX: Regex =
        Regex::new(r"reorder\s(?P<percent>[\d\.]+)%(\s(?P<correlation>[\d\.]+)%)?(.*\sgap\s(?P<distance>[\d]+))?")
            .unwrap();
}

impl FromStr for Reorder {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = REORDER_REGEX.captures(s) {
            let percent: Percentage = captures
                .name("percent")
                .ok_or("get duplicate percent error")?
                .as_str()
                .parse()?;

            let correlation: Option<Percentage> = match captures.name("correlation") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            let distance: Option<u32> = match captures.name("distance") {
                Some(s) => s.as_str().parse().ok(),
                None => None,
            };

            Ok(Reorder {
                percent,
                correlation,
                distance,
            })
        } else {
            Err("no duplicate".into())
        }
    }
}

/// RATE := rate RATE [ PACKETOVERHEAD [ CELLSIZE [ CELLOVERHEAD ]]]]
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
struct Rate {
    rate: u64,
    // TODO: [ PACKETOVERHEAD [ CELLSIZE [ CELLOVERHEAD ]]
}

impl Control for Rate {
    fn to_args(&self) -> Vec<String> {
        vec!["rate".into(), format!("{}bit", self.rate)]
    }
}

lazy_static! {
    static ref RATE_REGEX: Regex =
        Regex::new(r"rate\s(?P<number>[\d\.]+)(?P<unit>[KMGT]?bit)").unwrap();
}

impl FromStr for Rate {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(captures) = RATE_REGEX.captures(s) {
            let number: u64 = captures
                .name("number")
                .ok_or("get rate number error")?
                .as_str()
                .parse()?;

            let rate = match captures.name("unit").ok_or("get rate unit error")?.as_str() {
                "bit" => Some(number),
                "Kbit" => number.checked_mul(1000),
                "Mbit" => number.checked_mul(1000_000),
                "Gbit" => number.checked_mul(1000_000_000),
                "Tbit" => number.checked_mul(1000_000_000_000),
                unit => return Err(format!("error unit: {}", unit).into()),
            }
            .unwrap_or(u64::max_value());

            Ok(Rate { rate })
        } else {
            Err("no rate".into())
        }
    }
}

// TODO: SLOT := slot { MIN_DELAY [ MAX_DELAY ] |
//                      distribution { uniform | normal | pareto |
//       paretonormal | FILE } DELAY JITTER }
//                    [ packets PACKETS ] [ bytes BYTES ]

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Controls {
    #[serde(skip_serializing_if = "Option::is_none")]
    limit: Option<Limit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delay: Option<Delay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loss: Option<Loss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    corrupt: Option<Corrupt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duplicate: Option<Duplicate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reorder: Option<Reorder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate: Option<Rate>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            limit: None,
            delay: None,
            loss: None,
            duplicate: None,
            reorder: None,
            corrupt: None,
            rate: None,
        }
    }
}

impl Control for Controls {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::new();

        if let Some(limit) = &self.limit {
            v.append(&mut limit.to_args());
        }

        if let Some(delay) = &self.delay {
            v.append(&mut delay.to_args());
        }

        if let Some(loss) = &self.loss {
            v.append(&mut loss.to_args());
        }

        if let Some(duplicate) = &self.duplicate {
            v.append(&mut duplicate.to_args());
        }

        if self.delay.is_some() {
            if let Some(reorder) = &self.reorder {
                // to use reordering, a delay option must be specified.
                v.append(&mut reorder.to_args());
            }
        }

        if let Some(corrupt) = &self.corrupt {
            v.append(&mut corrupt.to_args());
        }

        if let Some(rate) = &self.rate {
            v.append(&mut rate.to_args());
        }

        v
    }
}

impl FromStr for Controls {
    type Err = Exception;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !s.starts_with("qdisc netem") {
            return Ok(Controls::default());
        }

        let limit = Limit::from_str(s).ok();
        let delay = Delay::from_str(s).ok();
        let loss = Loss::from_str(s).ok();
        let duplicate = Duplicate::from_str(s).ok();
        let reorder = Reorder::from_str(s).ok();
        let corrupt = Corrupt::from_str(s).ok();
        let rate = Rate::from_str(s).ok();

        Ok(Controls {
            limit,
            delay,
            loss,
            corrupt,
            duplicate,
            reorder,
            rate,
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum NetEm {
    #[serde(rename = "set")]
    Set {
        interface: String,
        controls: Controls,
    },
    #[serde(rename = "show")]
    Show { interface: String },
    // list all names of interfaces
    #[serde(rename = "list")]
    List,
    #[serde(rename = "reset")]
    Reset { interface: String },
}

fn output_to_interfaces(output: &str) -> Vec<String> {
    lazy_static! {
        static ref INTERFACE_REGEX: Regex =
            Regex::new(r"^qdisc\s.*:\sdev\s(?P<interface>.*)\sroot").unwrap();
    }

    output
        .lines()
        .map(|s| INTERFACE_REGEX.captures(s))
        .filter_map(|o| o)
        .map(|c| c.name("interface"))
        .filter_map(|o| o)
        .map(|m| m.as_str().to_owned())
        .collect::<Vec<String>>()
}

impl NetEm {
    pub async fn execute(&self) -> Output {
        let args = self.to_args();
        println!("run => tc {}", args.join(" "));
        match Command::new("tc").args(args).output().await {
            Ok(output) => match output.status.code() {
                Some(code) => {
                    if code == 0 {
                        match String::from_utf8(output.stdout) {
                            Ok(stdout) => match self {
                                NetEm::Show { interface } => match Controls::from_str(&stdout) {
                                    Ok(controls) => Output::Controls {
                                        interface: interface.into(),
                                        controls,
                                    },
                                    Err(e) => Output::err_server(format!(
                                        "Parse output to contorls error: {}",
                                        e
                                    )),
                                },
                                NetEm::List => Output::Interfaces {
                                    list: output_to_interfaces(&stdout),
                                },
                                _ => Output::Ok,
                            },
                            Err(e) => Output::err_server(format!(
                                "Process output decode(utf8) error: {}",
                                e
                            )),
                        }
                    } else {
                        let description = match String::from_utf8(output.stderr) {
                            Ok(stderr) => {
                                format!("Exit with status code: {}, stderr: {}", code, stderr)
                            }
                            Err(_) => format!("Exit with status code: {}", code),
                        };
                        Output::err_server(description)
                    }
                }
                None => Output::err_server("Process killed by signal".to_owned()),
            },
            Err(e) => Output::err_server(format!("Command error: {}", e)),
        }
    }
}

impl Control for NetEm {
    fn to_args(&self) -> Vec<String> {
        match self {
            NetEm::Set {
                interface,
                controls,
            } => {
                // tc qdisc replace dev <INTERFACE> root netem delay 100ms 10ms loss 1% 30% duplicate 1% reorder 10% 50% corrupt 0.2%
                let mut args = vec![
                    "qdisc".into(),
                    "replace".into(),
                    "dev".into(),
                    interface.into(),
                    "root".into(),
                    "netem".into(),
                ];

                args.append(&mut controls.to_args());

                args
            }
            NetEm::Show { interface } => {
                // tc qdisc show dev <INTERFACE>
                vec![
                    "qdisc".into(),
                    "show".into(),
                    "dev".into(),
                    interface.into(),
                ]
            }
            NetEm::Reset { interface } => {
                // tc qdisc del dev <INTERFACE> root netem
                vec![
                    "qdisc".into(),
                    "del".into(),
                    "dev".into(),
                    interface.into(),
                    "root".into(),
                    "netem".into(),
                ]
            }
            NetEm::List => vec!["qdisc".into(), "show".into()],
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum Output {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "controls")]
    Controls {
        interface: String,
        controls: Controls,
    },
    #[serde(rename = "interfaces")]
    Interfaces { list: Vec<String> },
    #[serde(rename = "error")]
    Error { description: String, server: bool },
}

impl Output {
    pub fn err_server(description: String) -> Self {
        Output::Error {
            description,
            server: true,
        }
    }

    pub fn err_client(description: String) -> Self {
        Output::Error {
            description,
            server: false,
        }
    }
}

#[test]
pub fn test_netem() {
    let control = NetEm::Set {
        interface: "br-lan".to_owned(),
        controls: Controls {
            limit: Some(Limit { packets: 2000 }),
            delay: Some(Delay {
                time: 10.0,
                jitter: Some(2.0),
                correlation: Some(50.0),
                distribution: None,
            }),
            loss: Some(Loss {
                percent: 0.1,
                correlation: Some(11.0),
                ecn: true,
            }),
            duplicate: Some(Duplicate {
                percent: 0.1,
                correlation: Some(12.0),
            }),
            reorder: Some(Reorder {
                percent: 10.0,
                correlation: Some(55.0),
                distance: Some(5),
            }),
            corrupt: Some(Corrupt {
                percent: 0.3,
                correlation: Some(30.0),
            }),
            rate: Some(Rate { rate: 10000 }),
        },
    };

    let show = NetEm::Show {
        interface: "br-lan".into(),
    };

    let list = NetEm::List;

    let reset = NetEm::Reset {
        interface: "br-lan".into(),
    };

    let print = |ne: &NetEm| {
        println!(
            "{} \n=> tc {}",
            serde_json::to_string_pretty(&ne).unwrap(),
            ne.to_args().join(" ")
        );
    };

    print(&control);
    print(&show);
    print(&list);
    print(&reset);
}

#[test]
fn test_regex() -> crate::error::WeoResult<()> {
    let is_netem = regex::Regex::new(r"^qdisc\snetem\s\d+:.*")?;

    let output = "qdisc netem 8018: root refcnt 2 limit 1000 delay 10.0ms  2.0ms 50% loss 0.1% 11% duplicate 0.1% 12% reorder 10% 55% corrupt 0.3% 30% rate 10Mbit ecn  gap 5";

    assert!(is_netem.is_match(output));

    let start = std::time::Instant::now();

    let limit = output.parse::<Limit>()?;
    println!("{:?}", limit);

    let delay = output.parse::<Delay>()?;
    println!("{:?}", delay);

    let loss = output.parse::<Loss>()?;
    println!("{:?}", loss);

    let duplicate = output.parse::<Duplicate>()?;
    println!("{:?}", duplicate);

    let reorder = output.parse::<Reorder>()?;
    println!("{:?}", reorder);

    let corrupt = output.parse::<Corrupt>()?;
    println!("{:?}", corrupt);

    let rate = output.parse::<Rate>()?;
    println!("{:?}", rate);

    let controls = Controls {
        limit: Some(limit),
        delay: Some(delay),
        loss: Some(loss),
        corrupt: Some(corrupt),
        duplicate: Some(duplicate),
        reorder: Some(reorder),
        rate: Some(rate),
    };

    println!("{}", serde_json::to_string_pretty(&controls)?);

    println!("{:?}", start.elapsed());

    let list = r"qdisc noqueue 0: dev lo root refcnt 2
qdisc fq_codel 0: dev eth0 root refcnt 2 limit 10240p flows 1024 quantum 1514 target 5.0ms interval 100.0ms memory_limit 4Mb ecn
qdisc noqueue 0: dev br-lan root refcnt 2
qdisc noqueue 0: dev eth0.1 root refcnt 2
qdisc noqueue 0: dev eth0.2 root refcnt 2
qdisc noqueue 0: dev wlan0 root refcnt 2
qdisc noqueue 0: dev wlan1 root refcnt 2";

    println!("{:?}", output_to_interfaces(list));

    Ok(())
}
