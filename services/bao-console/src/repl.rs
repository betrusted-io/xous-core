use xous::MessageEnvelope;

use crate::CmdEnv;

#[derive(Debug)]
#[allow(dead_code)] // until we implement history processing...
pub(crate) struct History {
    // the history record
    pub text: String,
    // if true, this was input from the user; if false, it's a response from the shell
    pub is_input: bool,
}

pub const HISTORY_DEPTH: usize = 10;
pub(crate) struct Repl {
    // optional structures that indicate new input to the Repl loop per iteration
    // an input string
    input: Option<String>,
    // messages from other servers
    msg: Option<MessageEnvelope>,

    // record our input history
    history: Vec<History>,
    history_len: usize,

    // command environment
    env: CmdEnv,
}
impl Repl {
    pub(crate) fn new(xns: &xous_names::XousNames) -> Self {
        Repl {
            input: None,
            msg: None,
            history: Vec::new(),
            history_len: HISTORY_DEPTH,
            env: CmdEnv::new(xns),
        }
    }

    /// accept a new input string
    pub(crate) fn input(&mut self, line: &str) -> Result<(), xous::Error> {
        self.input = Some(String::from(line));

        Ok(())
    }

    pub(crate) fn msg(&mut self, message: MessageEnvelope) { self.msg = Some(message); }

    pub(crate) fn circular_push(&mut self, item: History) {
        if self.history.len() >= self.history_len {
            self.history.remove(0);
        }
        self.history.push(item);
    }

    // An index of -1 gives the most recent command.
    // -2 would give the second most recent command
    pub(crate) fn get_history(&mut self, index: isize) -> &str {
        &self.history
            [(self.history.len() as isize + index).max(0).min(self.history.len() as isize - 1) as usize]
            .text
    }

    /// update the loop, in response to various inputs
    pub(crate) fn update(&mut self, _was_callback: bool, _init_done: bool) -> Result<(), xous::Error> {
        // if we had an input string, do something
        if let Some(local) = &self.input {
            let input_history = History { text: local.to_string(), is_input: true };
            self.circular_push(input_history);
        }

        // AT THIS POINT: if we have other inputs, update accordingly
        // other inputs might be, for example, events that came in from other servers that would
        // side effect our commands
        // take the input and pass it on to the various command parsers, and attach result
        if let Some(local) = &mut self.input {
            println!("[console] {}", local);
            if let Some(res) = self.env.dispatch(Some(local), None).expect("command dispatch failed") {
                println!("{}", res);
            }
        } else if let Some(msg) = &self.msg {
            log::trace!("processing callback msg: {:?}", msg);
            if let Some(res) = self.env.dispatch(None, Some(msg)).expect("callback failed") {
                println!("{}", res);
            }
        }

        // clear all the inputs to the loop, so we don't process them twice
        self.input = None;
        self.msg = None;

        Ok(())
    }
}
