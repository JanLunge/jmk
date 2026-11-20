//! Keeping the keymap of two halves the same.
//!
//! Both halves hold the whole keymap. Each tells the other a digest of its
//! own, the digest the two last agreed on (its base) and how often it was
//! saved with a change (its revision). From that each half works out, alone
//! and to the same result, which keymap is the newer one; the half with the
//! older one fetches it over the cable and saves it. Only saved keymaps
//! travel, so nothing half edited ever lands on the other half.
//!
//! A keymap is newer when it was changed since the halves agreed and the
//! other was not. If both were changed there is no telling which the user
//! wants, and nothing moves until they say.

/// What one half says about its keymap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Side {
    pub digest: u32,
    pub base: Option<u32>,
    pub revision: u32,
    /// The keymap is saved as it is.
    pub clean: bool,
    /// The user chose this half's keymap for both.
    pub force: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    Same,
    /// The other half's keymap is the newer one: fetch it.
    Fetch,
    /// This half's is: the other half fetches it.
    Offer,
    Conflict,
}

pub(crate) fn decide(me: &Side, peer: &Side) -> Verdict {
    if me.digest == peer.digest {
        return Verdict::Same;
    }
    if me.force != peer.force {
        return if peer.force { Verdict::Fetch } else { Verdict::Offer };
    }
    // What the other half last agreed on is exactly what one half still
    // holds: that half is the old one. This also holds for a half that lost
    // track of its own base.
    let i_am_old = peer.base == Some(me.digest);
    let peer_is_old = me.base == Some(peer.digest);
    if i_am_old != peer_is_old {
        return if i_am_old { Verdict::Fetch } else { Verdict::Offer };
    }
    let changed = |side: &Side| side.base.is_some_and(|base| base != side.digest);
    match (changed(me), changed(peer)) {
        (true, false) => Verdict::Offer,
        (false, true) => Verdict::Fetch,
        (true, true) => Verdict::Conflict,
        // Two halves that never agreed on anything: one that was never
        // given a keymap takes the other's.
        (false, false) => match (me.revision, peer.revision) {
            (0, 1..) => Verdict::Fetch,
            (1.., 0) => Verdict::Offer,
            _ => Verdict::Conflict,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn side(digest: u32, base: Option<u32>, revision: u32) -> Side {
        Side { digest, base, revision, clean: true, force: false }
    }

    /// Both halves must come to matching conclusions.
    fn both(a: Side, b: Side) -> Verdict {
        let (mine, theirs) = (decide(&a, &b), decide(&b, &a));
        let mirrored = match theirs {
            Verdict::Fetch => Verdict::Offer,
            Verdict::Offer => Verdict::Fetch,
            other => other,
        };
        assert_eq!(mine, mirrored);
        mine
    }

    #[test]
    fn the_half_that_was_changed_is_the_newer_one() {
        assert_eq!(both(side(9, Some(8), 9), side(8, Some(8), 8)), Verdict::Offer);
        assert_eq!(both(side(8, Some(8), 8), side(9, Some(8), 9)), Verdict::Fetch);
        // Saved several times while the other was away.
        assert_eq!(both(side(11, Some(8), 11), side(8, Some(8), 8)), Verdict::Offer);
    }

    #[test]
    fn two_changed_halves_are_a_conflict_whatever_their_revisions() {
        assert_eq!(both(side(9, Some(8), 9), side(7, Some(8), 9)), Verdict::Conflict);
        assert_eq!(both(side(9, Some(8), 12), side(7, Some(8), 9)), Verdict::Conflict);
    }

    #[test]
    fn a_half_that_forgot_its_base_still_gets_it_right() {
        // The first half agreed on 8 but only remembers 7.
        assert_eq!(both(side(8, Some(7), 8), side(9, Some(8), 9)), Verdict::Fetch);
        assert_eq!(both(side(9, Some(7), 9), side(8, Some(8), 8)), Verdict::Offer);
    }

    #[test]
    fn a_half_without_a_keymap_takes_the_others() {
        assert_eq!(both(side(1, None, 0), side(9, Some(9), 4)), Verdict::Fetch);
        assert_eq!(both(side(1, None, 0), side(9, None, 4)), Verdict::Fetch);
        assert_eq!(both(side(1, None, 2), side(9, None, 4)), Verdict::Conflict);
    }

    #[test]
    fn the_users_choice_settles_a_conflict() {
        let mut mine = side(9, Some(8), 9);
        let theirs = side(7, Some(8), 9);
        mine.force = true;
        assert_eq!(both(mine, theirs), Verdict::Offer);
    }
}
