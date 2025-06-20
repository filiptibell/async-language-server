impl super::RangeExt for std::ops::Range<usize> {
    type Position = usize;

    fn split_at(self, at: Self::Position) -> (Self, Self) {
        assert!(at <= self.end - self.start);
        let left = self.start..(self.start + at);
        let right = (self.start + at)..self.end;
        (left, right)
    }

    fn sub(self, from: Self::Position, to: Self::Position) -> Self {
        assert!(from <= self.end - self.start);
        assert!(to <= self.end - self.start);
        assert!(from <= to);
        (self.start + from)..(self.start + to)
    }

    fn sub_delimited(self, text: &str, delim: char) -> (Option<Self>, Option<Self>) {
        assert_eq!(
            text.len(),
            self.end - self.start,
            "text and range must be the same length"
        );
        assert_eq!(
            delim.len_utf8(),
            1,
            "delim must be a single-byte UTF8 character"
        );

        if let Some(offset) = text.find(delim) {
            (
                if offset == 0 {
                    None // delimiter is the first character
                } else {
                    Some(self.clone().split_off_left(offset))
                },
                if offset + 1 >= text.len() {
                    None // delimiter is the last character
                } else {
                    Some(self.clone().split_off_right(offset + 1))
                },
            )
        } else if !text.is_empty() {
            (Some(self.clone()), None)
        } else {
            (None, None)
        }
    }

    fn sub_delimited_tri(
        self,
        text: &str,
        delim0: char,
        delim1: char,
    ) -> (Option<Self>, Option<Self>, Option<Self>) {
        assert_eq!(
            delim0.len_utf8(),
            1,
            "delim0 must be a single-byte UTF8 character"
        );
        assert_eq!(
            delim1.len_utf8(),
            1,
            "delim1 must be a single-byte UTF8 character"
        );

        if text.is_empty() {
            return (None, None, None);
        }

        assert_eq!(
            text.len(),
            self.end - self.start,
            "text and range must be the same length"
        );

        let (first, remainder) = self.clone().sub_delimited(text, delim0);

        if let Some(remainder) = remainder {
            // Extract the text corresponding to the remainder range
            let remainder_start = remainder.start - self.start;
            let remainder_end = remainder.end - self.start;
            let remainder_text = &text[remainder_start..remainder_end];

            // Split the remainder on the second delimiter
            let (second, third) = remainder.sub_delimited(remainder_text, delim1);
            (first, second, third)
        } else {
            (first, None, None)
        }
    }
}
