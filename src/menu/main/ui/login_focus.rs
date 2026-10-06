#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Focus {
    Username,
    Password,
    Server,
    Multiplayer,
}

pub(super) fn next(current: Option<Focus>, backwards: bool, server_locked: bool) -> Focus {
    let order: &[Focus] = if server_locked {
        &[Focus::Username, Focus::Password, Focus::Multiplayer]
    } else {
        &[
            Focus::Username,
            Focus::Password,
            Focus::Server,
            Focus::Multiplayer,
        ]
    };
    let Some(index) = order.iter().position(|focus| Some(*focus) == current) else {
        return if backwards {
            *order.last().unwrap()
        } else {
            order[0]
        };
    };
    order[(index + if backwards { order.len() - 1 } else { 1 }) % order.len()]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forward_and_reverse_include_button_and_wrap() {
        let order = [
            Focus::Username,
            Focus::Password,
            Focus::Server,
            Focus::Multiplayer,
        ];
        for (index, focus) in order.into_iter().enumerate() {
            assert_eq!(next(Some(focus), false, false), order[(index + 1) % 4]);
            assert_eq!(next(Some(focus), true, false), order[(index + 3) % 4]);
        }
        assert_eq!(next(None, false, false), Focus::Username);
        assert_eq!(next(None, true, false), Focus::Multiplayer);
    }
    #[test]
    fn locked_server_is_skipped_in_both_directions() {
        assert_eq!(next(Some(Focus::Password), false, true), Focus::Multiplayer);
        assert_eq!(next(Some(Focus::Multiplayer), true, true), Focus::Password);
    }
}
