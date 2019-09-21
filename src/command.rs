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

// refer to: http://man7.org/linux/man-pages/man8/tc-netem.8.html
trait Control {
    fn to_args(&self) -> Vec<String>;
}

#[derive(Serialize, Deserialize, Debug)]
struct Delay {
    duration: Millisecond,
    #[serde(skip_serializing_if = "Option::is_none")]
    jitter: Option<Millisecond>,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
    // ? distribution
}

impl Control for Delay {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(3);

        v.push("delay".into());
        v.push(self.duration.to_ms_string());

        if let Some(jitter) = self.jitter {
            v.push(jitter.to_ms_string());
        }

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Loss {
    prob: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    random: Option<Percentage>,
}

impl Control for Loss {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("loss".into());
        v.push(self.prob.to_pct_string());

        if let Some(random) = self.random {
            v.push(random.to_pct_string());
        }

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Corrupt {
    prob: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
}

impl Control for Corrupt {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("corrupt".into());
        v.push(self.prob.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Duplicate {
    prob: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
}

impl Control for Duplicate {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("duplicate".into());
        v.push(self.prob.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct Reorder {
    prob: Percentage,
    #[serde(skip_serializing_if = "Option::is_none")]
    correlation: Option<Percentage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gap: Option<u32>,
}

impl Control for Reorder {
    fn to_args(&self) -> Vec<String> {
        let mut v = Vec::with_capacity(2);

        v.push("reorder".into());
        v.push(self.prob.to_pct_string());

        if let Some(correlation) = self.correlation {
            v.push(correlation.to_pct_string());
        }

        if let Some(gap) = self.gap {
            v.push("gap".to_owned());
            v.push(gap.to_string())
        }

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Controls {
    #[serde(skip_serializing_if = "Option::is_none")]
    delay: Option<Delay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loss: Option<Loss>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duplicate: Option<Duplicate>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reorder: Option<Reorder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    corrupt: Option<Corrupt>,
}

impl Default for Controls {
    fn default() -> Self {
        Self {
            delay: None,
            loss: None,
            duplicate: None,
            reorder: None,
            corrupt: None,
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

        v
    }
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "command")]
pub enum Tc {
    #[serde(rename = "control")]
    Control {
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

impl Tc {
    pub async fn execute(&self) -> WeoResult<Message> {
        println!("execute {:?}", self);
        let output = PsCommand::new("tc").args(self.get_args()?).output().await?;

        match output.status.code() {
            Some(code) => {
                if code == 0 {
                    Ok(Message::ok())
                } else {
                    Ok(Message::err(format!("Exit with status code: {}", code)))
                }
            }
            None => Ok(Message::err("Process killed by signal".to_owned())),
        }
    }

    pub fn get_args(&self) -> WeoResult<Vec<String>> {
        match self {
            Tc::Control {
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
            Tc::Show { interface: if_op } => {
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
            Tc::Reset { interface } => {
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
pub struct Message {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
}

impl Message {
    pub fn ok() -> Self {
        Message {
            ok: true,
            message: None,
        }
    }

    pub fn err(message: String) -> Self {
        Message {
            ok: false,
            message: Some(message),
        }
    }
}

pub fn fuck() {
    let control = Tc::Control {
        controls: Controls {
            delay: Some(Delay {
                duration: 10,
                jitter: Some(12),
                correlation: None,
            }),
            ..Default::default()
        },
        interface: "br-lan".to_owned(),
    };

    let show = Tc::Show {
        interface: Some("br-lan".into()),
    };

    let show_all = Tc::Show { interface: None };

    let reset = Tc::Reset {
        interface: "br-lan".into(),
    };

    println!("{}", serde_json::to_string(&control).unwrap());
    println!("{}", control.get_args().unwrap_or_else(|_| Vec::new()).join(" "));
    println!("{}", serde_json::to_string(&show).unwrap());
    println!("{}", show.get_args().unwrap_or_else(|_| Vec::new()).join(" "));
    println!("{}", serde_json::to_string(&show_all).unwrap());
    println!("{}", show_all.get_args().unwrap_or_else(|_| Vec::new()).join(" "));
    println!("{}", serde_json::to_string(&reset).unwrap());
    println!("{}", reset.get_args().unwrap_or_else(|_| Vec::new()).join(" "));

    std::process::exit(0);
}
