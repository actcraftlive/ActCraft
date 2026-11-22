//! How playback moves through a part: repeats, first and second endings, D.C., D.S., Fine and
//! Coda. [`play_order`] unrolls the written bars into the order they are played.
//!
//! The rules follow common practice as MusicXML describes it: a backward repeat goes back to the
//! last forward repeat (or the start, or the bar after the previous repeat or ending); an ending
//! plays on the passes its numbers name; after a D.C. or D.S. jump repeats are not taken again,
//! endings play their last pass, Fine stops and To Coda goes to the coda.

use crate::Bar;

/// Most times one repeated section plays.
pub const MAX_REPEAT_TIMES: u8 = 16;
/// Ending numbers kept per bar.
pub const MAX_ENDING_NUMBERS: usize = 8;
/// Bars in the played order at most (a hostile file cannot make it longer).
pub const MAX_PLAYED: usize = 50_000;

/// Repeat signs, endings and jumps at one bar.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Flow {
    /// A repeated section starts at this bar (a forward repeat sign at its left).
    pub repeat_start: bool,
    /// A repeated section ends at this bar: how many times it plays (2 for a plain repeat sign).
    pub repeat_end: Option<u8>,
    /// The ending (first, second... ending) numbers this bar belongs to; empty when none.
    pub endings: Vec<u8>,
    /// An ending's bracket starts at this bar.
    pub ending_start: bool,
    /// The segno sign: where a D.S. goes back to.
    pub segno: bool,
    /// The coda starts here: where To Coda goes.
    pub coda: bool,
    /// Go to the coda after this bar, once a D.C. or D.S. was taken.
    pub to_coda: bool,
    /// Go back to the start after this bar (D.C.).
    pub da_capo: bool,
    /// Go back to the segno after this bar (D.S.).
    pub dal_segno: bool,
    /// The piece ends after this bar, once a D.C. or D.S. was taken.
    pub fine: bool,
}

impl Flow {
    /// Nothing here changes the order bars are played in.
    pub fn is_plain(&self) -> bool {
        self == &Flow::default()
    }
}

/// The bars of a part in the order they are played, as indexes into `bars`. Without repeats or
/// jumps that is every bar once.
pub fn play_order(bars: &[Bar]) -> Vec<usize> {
    if bars.iter().all(|b| b.flow.is_plain()) {
        return (0..bars.len()).collect();
    }
    // For each bar in an ending, the highest ending number of its group of endings: the one
    // played after a jump.
    let mut last_ending = vec![0u8; bars.len()];
    let mut i = 0;
    while i < bars.len() {
        let mut j = i;
        let mut most = 0;
        while let Some(b) = bars.get(j).filter(|b| !b.flow.endings.is_empty()) {
            most = b.flow.endings.iter().copied().fold(most, u8::max);
            j += 1;
        }
        for slot in last_ending.iter_mut().take(j).skip(i) {
            *slot = most;
        }
        i = j.max(i + 1);
    }

    let mut order = Vec::new();
    let mut i = 0;
    // Where the repeated section starts, and which pass through it this is.
    let mut start = 0;
    let mut pass: u8 = 1;
    let mut jumped = false;
    let mut coming_back = false;
    let mut in_ending = false;
    let mut steps = 0usize;
    while let Some(bar) = bars.get(i) {
        steps += 1;
        if order.len() >= MAX_PLAYED || steps > 4 * MAX_PLAYED {
            break;
        }
        let flow = &bar.flow;
        if flow.repeat_start && !coming_back && !jumped {
            start = i;
            pass = 1;
        }
        coming_back = false;
        if !flow.endings.is_empty() {
            let wanted = if jumped { last_ending.get(i).copied().unwrap_or(0) } else { pass };
            if !flow.endings.contains(&wanted) {
                i += 1;
                continue;
            }
        } else if in_ending {
            // Past the endings: what follows is a new section.
            start = i;
            pass = 1;
        }
        in_ending = !flow.endings.is_empty();
        order.push(i);
        if jumped && flow.fine {
            break;
        }
        if jumped
            && flow.to_coda
            && let Some(coda) = bars.iter().skip(i + 1).position(|b| b.flow.coda).map(|k| k + i + 1)
        {
            i = coda;
            continue;
        }
        if !jumped && let Some(times) = flow.repeat_end {
            if pass < times.clamp(2, MAX_REPEAT_TIMES) {
                pass += 1;
                i = start;
                coming_back = true;
                in_ending = false;
                continue;
            }
            if flow.endings.is_empty() {
                start = i + 1;
                pass = 1;
            }
        }
        if !jumped && (flow.da_capo || flow.dal_segno) {
            jumped = true;
            let target = if flow.dal_segno { bars.iter().position(|b| b.flow.segno).unwrap_or(0) } else { 0 };
            i = target;
            start = target;
            pass = 1;
            in_ending = false;
            continue;
        }
        i += 1;
    }
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(flows: Vec<Flow>) -> Vec<Bar> {
        flows
            .into_iter()
            .enumerate()
            .map(|(i, flow)| Bar {
                number: (i + 1).to_string(),
                beats: 4,
                beat_type: 4,
                fifths: 0,
                tempo: None,
                length: 3840,
                notes: Vec::new(),
                flow,
            })
            .collect()
    }

    fn plain() -> Flow {
        Flow::default()
    }

    fn ending(numbers: &[u8]) -> Flow {
        Flow { endings: numbers.to_vec(), ending_start: true, ..Flow::default() }
    }

    #[test]
    fn plain_bars_play_once_in_order() {
        assert_eq!(play_order(&bars(vec![plain(), plain(), plain()])), vec![0, 1, 2]);
        assert_eq!(play_order(&[]), Vec::<usize>::new());
    }

    #[test]
    fn a_repeat_plays_its_section_again() {
        // |: 1 2 :| 3, and a repeat without a forward sign goes back to the start.
        let mut f = vec![plain(), plain(), plain()];
        f[0].repeat_start = true;
        f[1].repeat_end = Some(2);
        assert_eq!(play_order(&bars(f)), vec![0, 1, 0, 1, 2]);
        let mut f = vec![plain(), plain(), plain()];
        f[1].repeat_end = Some(3);
        assert_eq!(play_order(&bars(f)), vec![0, 1, 0, 1, 0, 1, 2]);
        // 1 |: 2 :| 3 :| goes back to the bar after the first repeat.
        let mut f = vec![plain(), plain(), plain(), plain()];
        f[0].repeat_end = Some(2);
        f[2].repeat_end = Some(2);
        assert_eq!(play_order(&bars(f)), vec![0, 0, 1, 2, 1, 2, 3]);
    }

    #[test]
    fn first_and_second_endings_play_on_their_pass() {
        // |: 1 2 [1. 3 :| [2. 4 | 5
        let mut f = vec![plain(), plain(), ending(&[1]), ending(&[2]), plain()];
        f[0].repeat_start = true;
        f[2].repeat_end = Some(2);
        assert_eq!(play_order(&bars(f)), vec![0, 1, 2, 0, 1, 3, 4]);
        // Three passes: [1.-2. 3 :| [3. 4
        let mut f = vec![plain(), ending(&[1, 2]), ending(&[3])];
        f[1].repeat_end = Some(3);
        assert_eq!(play_order(&bars(f)), vec![0, 1, 0, 1, 0, 2]);
    }

    #[test]
    fn da_capo_al_fine_and_dal_segno_al_coda() {
        // 1 2(Fine) 3(D.C.): the repeat isn't taken again after the jump.
        let mut f = vec![plain(), plain(), plain()];
        f[1].fine = true;
        f[2].da_capo = true;
        assert_eq!(play_order(&bars(f)), vec![0, 1, 2, 0, 1]);
        // 1 2(segno) 3(To Coda) 4(D.S.) 5(Coda)
        let mut f = vec![plain(), plain(), plain(), plain(), plain()];
        f[1].segno = true;
        f[2].to_coda = true;
        f[3].dal_segno = true;
        f[4].coda = true;
        assert_eq!(play_order(&bars(f)), vec![0, 1, 2, 3, 1, 2, 4]);
        // |: 1 [1. 2 :| [2. 3 | 4(D.C.): after the jump the second ending plays, once.
        let mut f = vec![plain(), ending(&[1]), ending(&[2]), plain()];
        f[0].repeat_start = true;
        f[1].repeat_end = Some(2);
        f[3].da_capo = true;
        assert_eq!(play_order(&bars(f)), vec![0, 1, 0, 2, 3, 0, 2, 3]);
    }

    #[test]
    fn hostile_flows_end() {
        // Every bar a repeat end and a jump: the order stays bounded.
        let f: Vec<Flow> = (0..200)
            .map(|_| Flow {
                repeat_start: true,
                repeat_end: Some(255),
                da_capo: true,
                dal_segno: true,
                segno: true,
                to_coda: true,
                coda: true,
                ..Flow::default()
            })
            .collect();
        let order = play_order(&bars(f));
        assert!(!order.is_empty() && order.len() <= MAX_PLAYED);
        // Endings no pass reaches are skipped, not looped on.
        let f = vec![ending(&[5]), ending(&[6]), plain()];
        assert_eq!(play_order(&bars(f)), vec![2]);
    }
}
