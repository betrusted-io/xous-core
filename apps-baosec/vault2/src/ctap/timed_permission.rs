#![cfg_attr(rustfmt, rustfmt_skip)]
// Copyright2019-2021 Google LLC
//
// Licensed under the Apache License, Version 2 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::time::Instant;
use std::time::Duration;
#[cfg(feature="xous")]
use crate::env::Env;
#[cfg(feature="xous")]
use crate::api::user_presence::UserPresence;

#[derive(Debug, Copy, Clone)]
pub enum TimedPermission {
    Waiting,
    Granted(Instant),
}

impl TimedPermission {
    pub fn waiting() -> TimedPermission {
        TimedPermission::Waiting
    }

    pub fn granted(now: Instant, grant_duration: Duration) -> TimedPermission {
        // TODO: Should panic or saturate if the grant duration is too big.
        TimedPermission::Granted(now.checked_add(grant_duration).unwrap())
    }

    // Checks if the timeout is not reached, false for differing ClockValue frequencies.
    pub fn is_granted(&self, now: Instant) -> bool {
        if let TimedPermission::Granted(timeout) = self {
            return timeout.checked_duration_since(now).is_some();
        }
        false
    }

    // Consumes the state and returns the current new permission state at time "now".
    // Returns a new state for differing ClockValue frequencies.
    pub fn check_expiration(self, now: Instant) -> TimedPermission {
        if let TimedPermission::Granted(timeout) = self {
            if timeout.checked_duration_since(now).is_some() {
                return TimedPermission::Granted(timeout);
            }
        }
        TimedPermission::Waiting
    }
}

#[cfg(feature = "with_ctap1")]
#[derive(Debug)]
#[allow(dead_code)]
pub struct U2fUserPresenceState {
    // If user presence was recently requested, its timeout is saved here.
    needs_up: TimedPermission,
    // Button touch timeouts, while user presence is requested, are saved here.
    has_up: TimedPermission,
    // This is the timeout duration of user presence requests.
    request_duration: Duration,
    // This is the timeout duration of button touches.
    presence_duration: Duration,
}

#[cfg(feature = "with_ctap1")]
impl U2fUserPresenceState {
    pub fn new(
        request_duration: Duration,
        presence_duration: Duration,
    ) -> U2fUserPresenceState {
        U2fUserPresenceState {
            needs_up: TimedPermission::Waiting,
            has_up: TimedPermission::Waiting,
            request_duration,
            presence_duration,
        }
    }

    // Granting user presence is ignored if it needs activation, but waits. Also cleans up.
    pub fn grant_up(&mut self, _now: Instant) {
        #[cfg(not(feature="xous"))]
        self.check_expiration(now);
        #[cfg(not(feature="xous"))]
        if self.needs_up.is_granted(now) {
            self.needs_up = TimedPermission::Waiting;
            self.has_up = TimedPermission::granted(now, self.presence_duration);
        }
    }

    // This marks user presence as needed or uses it up if already granted. Also cleans up.
    #[cfg(feature="xous")]
    pub fn consume_up(&mut self, env: &mut impl Env, reason: String, app_id: [u8; 32]) -> bool {
        env.user_presence().poll_approval_ctap1(String::from(reason), app_id)
    }
    #[cfg(not(feature="xous"))]
    pub fn consume_up(&mut self, now: Instant) -> bool {
        self.check_expiration(now);
        if self.has_up.is_granted(now) {
            self.has_up = TimedPermission::Waiting;
            true
        } else {
            self.needs_up = TimedPermission::granted(now, self.request_duration);
            false
        }
    }

    // Returns if user presence was requested. Also cleans up.
    #[cfg(not(feature="xous"))]
    pub fn is_up_needed(&mut self, now: Instant) -> bool {
        self.check_expiration(now);
        self.needs_up.is_granted(now)
    }
    #[cfg(feature="xous")]
    pub fn is_up_needed(&mut self, env: &mut impl Env, _now: Instant) -> bool {
        env.user_presence().recently_requested()
    }

    // If you don't regularly call any other function, not cleaning up leads to overflow problems.
    #[cfg(not(feature="xous"))]
    pub fn check_expiration(&mut self, now: Instant) {
        self.needs_up = self.needs_up.check_expiration(now);
        self.has_up = self.has_up.check_expiration(now);
    }
    #[cfg(feature="xous")]
    #[allow(dead_code)]
    pub fn check_expiration(&mut self, _now: Instant) {
    }
}

#[cfg(feature = "with_ctap1")]
#[cfg(test)]
mod test {
    use super::*;

    fn zero() -> Instant {
        Instant::new(0)
    }

    fn big_positive() -> Instant {
        Instant::new(u64::MAX / 1000 - 1)
    }

    fn request_duration() -> Duration {
        Duration::from_millis(1000)
    }

    fn presence_duration() -> Duration {
        Duration::from_millis(1000)
    }

    fn epsilon() -> Duration {
        Duration::from_millis(1)
    }
    #[cfg(not(feature="xous"))]
    fn grant_up_when_needed(start_time: Instant) {
        let mut u2f_state = U2fUserPresenceState::new(request_duration(), presence_duration());
        assert!(!u2f_state.consume_up(start_time));
        assert!(u2f_state.is_up_needed(start_time));
        u2f_state.grant_up(start_time);
        assert!(u2f_state.consume_up(start_time));
        assert!(!u2f_state.consume_up(start_time));
    }

    #[cfg(not(feature="xous"))]
    fn need_up_timeout(start_time: Instant) {
        let mut u2f_state = U2fUserPresenceState::new(request_duration(), presence_duration());
        assert!(!u2f_state.consume_up(start_time));
        assert!(u2f_state.is_up_needed(start_time));
        // The timeout excludes equality, so it should be over at this instant.
        assert!(!u2f_state.is_up_needed(
            start_time
                .checked_add(presence_duration() + epsilon())
                .unwrap()
        ));
    }

    #[cfg(not(feature="xous"))]
    fn grant_up_timeout(start_time: Instant) {
        let mut u2f_state = U2fUserPresenceState::new(request_duration(), presence_duration());
        assert!(!u2f_state.consume_up(start_time));
        assert!(u2f_state.is_up_needed(start_time));
        u2f_state.grant_up(start_time);
        // The timeout excludes equality, so it should be over at this instant.
        assert!(!u2f_state.consume_up(
            start_time
                .checked_add(presence_duration() + epsilon())
                .unwrap()
        ));
    }

    #[test]
    fn test_grant_up_timeout() {
        grant_up_timeout(zero());
        grant_up_timeout(big_positive());
    }

    #[test]
    fn test_need_up_timeout() {
        need_up_timeout(zero());
        need_up_timeout(big_positive());
    }

    #[test]
    fn test_grant_up_when_needed() {
        grant_up_when_needed(zero());
        grant_up_when_needed(big_positive());
    }

    #[cfg(not(feature="xous"))]
    #[test]
    fn test_grant_up_without_need() {
        let mut u2f_state = U2fUserPresenceState::new(request_duration(), presence_duration());
        u2f_state.grant_up(zero());
        assert!(!u2f_state.is_up_needed(zero()));
        assert!(!u2f_state.consume_up(zero()));
    }
}
