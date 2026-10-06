use iced::{
    Clipboard, Element, Event, Hasher, Layout, Length, Point, Rectangle, Widget, layout, mouse,
};

/// Select the nearest map target while the cursor is inside the map.
pub struct ProximitySelect<'a, M, R: iced::Renderer> {
    content: Element<'a, M, R>,
    targets: Vec<Point>,
    on_hover: fn(Option<usize>) -> M,
    on_select: fn(usize) -> M,
}

impl<'a, M, R: iced::Renderer> ProximitySelect<'a, M, R> {
    pub fn new(
        content: Element<'a, M, R>,
        targets: Vec<Point>,
        on_hover: fn(Option<usize>) -> M,
        on_select: fn(usize) -> M,
    ) -> Self {
        Self {
            content,
            targets,
            on_hover,
            on_select,
        }
    }

    fn nearest(&self, layout: Layout<'_>, cursor: Point) -> Option<usize> {
        let bounds = layout.bounds();
        if !bounds.contains(cursor) {
            return None;
        }
        self.targets
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let distance = |p: &Point| {
                    (cursor.x - bounds.x - p.x).powi(2) + (cursor.y - bounds.y - p.y).powi(2)
                };
                distance(a).total_cmp(&distance(b))
            })
            .map(|(idx, _)| idx)
    }
}

impl<M, R: iced::Renderer> Widget<M, R> for ProximitySelect<'_, M, R> {
    fn width(&self) -> Length {
        self.content.width()
    }
    fn height(&self) -> Length {
        self.content.height()
    }
    fn layout(&self, renderer: &R, limits: &layout::Limits) -> layout::Node {
        self.content.layout(renderer, limits)
    }
    fn hash_layout(&self, state: &mut Hasher) {
        self.content.hash_layout(state);
    }
    fn draw(
        &self,
        renderer: &mut R,
        defaults: &R::Defaults,
        layout: Layout<'_>,
        cursor: Point,
        viewport: &Rectangle,
    ) -> R::Output {
        self.content
            .draw(renderer, defaults, layout, cursor, viewport)
    }
    fn on_event(
        &mut self,
        event: Event,
        layout: Layout<'_>,
        cursor: Point,
        _renderer: &R,
        _clipboard: &mut dyn Clipboard,
        messages: &mut Vec<M>,
    ) -> iced::event::Status {
        match event {
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                messages.push((self.on_hover)(self.nearest(layout, position)));
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                messages.push((self.on_hover)(None));
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(idx) = self.nearest(layout, cursor) {
                    messages.push((self.on_select)(idx));
                    return iced::event::Status::Captured;
                }
            }
            _ => {}
        }
        iced::event::Status::Ignored
    }
}

impl<'a, M: 'a, R: iced::Renderer + 'a> From<ProximitySelect<'a, M, R>> for Element<'a, M, R> {
    fn from(widget: ProximitySelect<'a, M, R>) -> Self {
        Element::new(widget)
    }
}
