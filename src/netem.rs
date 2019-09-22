/// NetEm Wrapper
/// NetEm - Network Emulator
///
/// NetEm is an enhancement of the Linux traffic control facilities that
/// allow to add delay, packet loss, duplication and more other
/// characteristics to packets outgoing from a selected network
/// interface. NetEm is built using the existing Quality Of Service (QOS)
/// and Differentiated Services (diffserv) facilities in the Linux
/// kernel.
use crate::error::WeoResult;
use serde_derive::{Deserialize, Serialize};
use tokio_net::process::Command as PsCommand;

type Percentage = f64;
type Millisecond = u64;

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
#[derive(Serialize, Deserialize, Debug)]
struct Limit {
    packets: i32,
}

impl Control for Limit {
    fn to_args(&self) -> Vec<String> {
        vec!["limit".into(), format!("{}", self.packets)]
    }
}

#[derive(Serialize, Deserialize, Debug, Copy, Clone)]
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
#[derive(Serialize, Deserialize, Debug)]
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

/// LOSS := loss { random PERCENT [ CORRELATION ]  |
///                state p13 [ p31 [ p32 [ p23 [ p14]]]] |
///                gemodel p [ r [ 1-h [ 1-k ]]] }  [ ecn ]
#[derive(Serialize, Deserialize, Debug)]
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

/// CORRUPT := corrupt PERCENT [ CORRELATION ]]
#[derive(Serialize, Deserialize, Debug)]
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

/// DUPLICATION := duplicate PERCENT [ CORRELATION ]]
#[derive(Serialize, Deserialize, Debug)]
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

/// REORDERING := reorder PERCENT [ CORRELATION ] [ gap DISTANCE ]
#[derive(Serialize, Deserialize, Debug)]
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

/// RATE := rate RATE [ PACKETOVERHEAD [ CELLSIZE [ CELLOVERHEAD ]]]]
#[derive(Serialize, Deserialize, Debug)]
struct Rate {
    rate: u64,
    // TODO: [ PACKETOVERHEAD [ CELLSIZE [ CELLOVERHEAD ]]
}

impl Control for Rate {
    fn to_args(&self) -> Vec<String> {
        vec!["rate".into(), format!("{}kbit", self.rate)]
    }
}

// TODO: SLOT := slot { MIN_DELAY [ MAX_DELAY ] |
//                      distribution { uniform | normal | pareto |
//       paretonormal | FILE } DELAY JITTER }
//                    [ packets PACKETS ] [ bytes BYTES ]

#[derive(Serialize, Deserialize, Debug)]
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

impl Controls {
    pub fn is_valid(&self) -> bool {
        // to use reordering, a delay option must be specified.
        self.reorder.is_none() || self.delay.is_some()
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

        if let Some(reorder) = &self.reorder {
            // need delay
            v.append(&mut reorder.to_args());
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

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type")]
pub enum NetEm {
    #[serde(rename = "set")]
    Set {
        interface: String,
        controls: Controls,
    },
    #[serde(rename = "show")]
    Show {
        #[serde(skip_serializing_if = "Option::is_none")]
        interface: Option<String>,
    },
    #[serde(rename = "reset")]
    Reset { interface: String },
}

impl NetEm {
    pub async fn execute(&self) -> Message {
        println!("execute {:?}", self);
        match self.get_args() {
            Ok(args) => match PsCommand::new("tc").args(args).output().await {
                Ok(output) => match output.status.code() {
                    Some(code) => {
                        if code == 0 {
                            match self {
                                NetEm::Show { interface: _ } => {
                                    match String::from_utf8(output.stdout) {
                                        Ok(stdout) => Message::Data {
                                            interfaces: stdout
                                                .lines()
                                                .map(|s| s.to_owned())
                                                .collect(),
                                        },
                                        Err(e) => Message::err_server(format!(
                                            "Process output decode(utf8) error: {}",
                                            e
                                        )),
                                    }
                                }
                                _ => Message::Ok,
                            }
                        } else {
                            let description = match String::from_utf8(output.stderr) {
                                Ok(stderr) => {
                                    format!("Exit with status code: {}, stderr: {}", code, stderr)
                                }
                                Err(_) => format!("Exit with status code: {}", code),
                            };
                            Message::err_server(description)
                        }
                    }
                    None => Message::err_server("Process killed by signal".to_owned()),
                },
                Err(e) => Message::err_server(format!("Command error: {}", e)),
            },
            Err(e) => Message::err_client(format!("{}", e)),
        }
    }

    pub fn get_args(&self) -> WeoResult<Vec<String>> {
        match self {
            NetEm::Set {
                interface,
                controls,
            } => {
                if !controls.is_valid() {
                    return Err("to use reordering, a delay option must be specified".into());
                }

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

                Ok(args)
            }
            NetEm::Show { interface: if_op } => {
                match if_op {
                    Some(interface) => {
                        // tc qdisc show dev <INTERFACE>
                        Ok(vec![
                            "qdisc".into(),
                            "show".into(),
                            "dev".into(),
                            interface.into(),
                        ])
                    }
                    None => {
                        // tc qdisc show
                        Ok(vec!["qdisc".into(), "show".into()])
                    }
                }
            }
            NetEm::Reset { interface } => {
                // tc qdisc del dev <INTERFACE> root netem
                Ok(vec![
                    "qdisc".into(),
                    "del".into(),
                    "dev".into(),
                    interface.into(),
                    "root".into(),
                    "netem".into(),
                ])
            }
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "status")]
pub enum Message {
    #[serde(rename = "ok")]
    Ok,
    #[serde(rename = "error")]
    Error { description: String, server: bool },
    #[serde(rename = "data")]
    Data { interfaces: Vec<String> },
}

impl Message {
    pub fn err_server(description: String) -> Self {
        Message::Error {
            description,
            server: true,
        }
    }

    pub fn err_client(description: String) -> Self {
        Message::Error {
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
            limit: Some(Limit { packets: 1000 }),
            delay: Some(Delay {
                time: 10,
                jitter: Some(2),
                correlation: Some(50.0),
                distribution: Some(Distribution::Uniform),
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
        interface: Some("br-lan".into()),
    };

    let show_all = NetEm::Show { interface: None };

    let reset = NetEm::Reset {
        interface: "br-lan".into(),
    };

    let print = |ne: &NetEm| {
        println!("{} \n=> tc {}", serde_json::to_string_pretty(&ne).unwrap(), ne.get_args().unwrap_or_else(|_| Vec::new()).join(" "));
    };

    print(&control);
    print(&show);
    print(&show_all);
    print(&reset);
}
