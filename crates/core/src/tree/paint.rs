//! Layout measurement and clipped painting of the retained tree.
use super::{Error, Id, Tree};
use crate::{Buffer, Canvas, Rect};
use std::sync::atomic::{AtomicU64, Ordering};
use taffy::{AvailableSpace, Dimension, Display, Size};

impl Tree {
    /// Compute layout and paint only when state or dimensions changed. Callers
    /// borrow the completed frame; repeated reads of an idle tree do no work.
    pub fn frame(&mut self, width: u16, height: u16) -> Result<&Buffer, Error> {
        if self.frame.area().width != width || self.frame.area().height != height {
            self.dirty = true;
        }
        if !self.dirty {
            return Ok(&self.frame);
        }
        if self
            .focus
            .is_some_and(|id| !self.visible(id) || !self.nodes[id].element.focusable())
        {
            self.focus(None)?;
        }
        // An inline frame at its natural height was laid out by `height`, and a
        // repaint alone, after a focus change for example, moved nothing.
        let natural = self.fresh == Some((width, None)) && self.natural == Some((width, height));
        if !natural && self.fresh != Some((width, Some(height))) {
            self.compute(width, Some(height))?;
            self.fresh = Some((width, Some(height)));
        }
        let mut frame = std::mem::replace(&mut self.frame, Buffer::new(0, 0));
        if frame.area().width != width || frame.area().height != height {
            frame = Buffer::new(width, height);
        } else {
            frame.clear();
        }
        self.paint(self.root, (0, 0), frame.area(), &mut frame)?;
        if let Some((from, to)) = self.selection() {
            frame.invert(from, to);
        }
        // A number no other frame has, so renderers can tell a frame they
        // already drew without comparing its cells.
        static NEXT: AtomicU64 = AtomicU64::new(1);
        frame.version = NEXT.fetch_add(1, Ordering::Relaxed);
        self.frame = frame;
        self.dirty = false;
        Ok(&self.frame)
    }

    /// The height the content wants at a width, for frames that grow with
    /// their content, such as inline sessions. Pass the result to `frame`.
    pub fn height(&mut self, width: u16) -> Result<u16, Error> {
        // An idle tree answers from the last pass, so asking every loop
        // iteration neither lays out nor repaints.
        if let Some((_, height)) = self.natural.filter(|(w, _)| *w == width) {
            return Ok(height);
        }
        self.compute(width, None)?;
        let height = self
            .layout
            .layout(self.nodes[self.root].layout)?
            .size
            .height;
        let height = height.clamp(0.0, f32::from(u16::MAX)) as u16;
        self.fresh = Some((width, None));
        self.natural = Some((width, height));
        self.dirty = true;
        Ok(height)
    }

    /// Lay out the root at a width, and at a height or else its natural one.
    fn compute(&mut self, width: u16, height: Option<u16>) -> Result<(), Error> {
        let root = self.nodes[self.root].layout;
        let size = Size {
            width: Dimension::length(f32::from(width)),
            height: height.map_or(Dimension::auto(), |h| Dimension::length(f32::from(h))),
        };
        // Setting a style invalidates layout, so a repaint at the same size,
        // after a focus change for example, reuses the computed one.
        if self.layout.style(root)?.size != size {
            let mut style = self.layout.style(root)?.clone();
            style.size = size;
            self.layout.set_style(root, style)?;
        }
        let nodes = &self.nodes;
        self.layout.compute_layout_with_measure(
            root,
            Size {
                width: AvailableSpace::Definite(f32::from(width)),
                height: height.map_or(AvailableSpace::MaxContent, |h| {
                    AvailableSpace::Definite(f32::from(h))
                }),
            },
            |inputs, _, context, style| {
                taffy::compute_leaf_layout(
                    inputs,
                    style,
                    |_, _| 0.0,
                    |known, available| {
                        let width = known.width.or(match available.width {
                            AvailableSpace::Definite(w) => Some(w),
                            AvailableSpace::MinContent => Some(1.0),
                            _ => None,
                        });
                        let (w, h) = context.and_then(|id| nodes.get(*id)).map_or((0, 0), |n| {
                            n.element.measure(width.map(|n| n.max(0.0) as u16))
                        });
                        Size {
                            width: known.width.unwrap_or(f32::from(w)),
                            height: known.height.unwrap_or(f32::from(h)),
                        }
                    },
                )
            },
        )?;
        Ok(())
    }

    fn paint(
        &mut self,
        id: Id,
        parent: (i32, i32),
        clip: Rect,
        buffer: &mut Buffer,
    ) -> Result<(), Error> {
        let node = &self.nodes[id];
        let layout = *self.layout.layout(node.layout)?;
        let origin = (
            parent.0.saturating_add(layout.location.x as i32),
            parent.1.saturating_add(layout.location.y as i32),
        );
        // A container may be taller than a cell coordinate can express; its
        // clip uses the full extent even though its own canvas saturates.
        let extent = (
            layout.size.width.max(0.0) as u32,
            layout.size.height.max(0.0) as u32,
        );
        let size = (
            extent.0.min(u32::from(u16::MAX)) as u16,
            extent.1.min(u32::from(u16::MAX)) as u16,
        );
        let bounds = clip_signed(origin, extent, clip);
        // Children never draw outside their parent, so a node with nothing
        // visible ends the walk: scrolled-away content costs nothing to paint.
        if self.layout.style(node.layout)?.display == Display::None
            || bounds.width == 0
            || bounds.height == 0
        {
            self.hide(id);
            return Ok(());
        }
        let node = &mut self.nodes[id];
        node.origin = origin;
        node.size = size;
        node.clip = bounds;
        node.element.paint(&mut Canvas {
            buffer,
            origin,
            size,
            clip: bounds,
            focused: self.focus == Some(id),
        });
        let inset = (layout.border.left as i32, layout.border.top as i32);
        let inner_extent = (
            (layout.size.width - layout.border.left - layout.border.right).max(0.0) as u32,
            (layout.size.height - layout.border.top - layout.border.bottom).max(0.0) as u32,
        );
        let inner_size = (
            inner_extent.0.min(u32::from(u16::MAX)) as u16,
            inner_extent.1.min(u32::from(u16::MAX)) as u16,
        );
        let inner = clip_signed(
            (origin.0 + inset.0, origin.1 + inset.1),
            inner_extent,
            bounds,
        );
        let content = (
            layout.scrollable_overflow_rect.right.max(0.0) as u32,
            layout.scrollable_overflow_rect.bottom.max(0.0) as u32,
        );
        let offset = node.element.viewport(inner_size, content);
        let offset = (
            i32::try_from(offset.0).unwrap_or(i32::MAX),
            i32::try_from(offset.1).unwrap_or(i32::MAX),
        );
        let layers = self.nodes[id].layered.then(|| self.layers(id));
        for index in 0..self.nodes[id].children.len() {
            let child = match &layers {
                Some(layers) => layers[index],
                None => self.nodes[id].children[index],
            };
            self.paint(
                child,
                (
                    origin.0.saturating_sub(offset.0),
                    origin.1.saturating_sub(offset.1),
                ),
                inner,
                buffer,
            )?;
        }
        self.nodes[id].element.overlay(&mut Canvas {
            buffer,
            origin,
            size,
            clip: bounds,
            focused: self.focus == Some(id),
        });
        Ok(())
    }
}

fn clip_signed(origin: (i32, i32), size: (u32, u32), clip: Rect) -> Rect {
    let left = i64::from(origin.0).max(i64::from(clip.x));
    let top = i64::from(origin.1).max(i64::from(clip.y));
    let right =
        (i64::from(origin.0) + i64::from(size.0)).min(i64::from(clip.x) + i64::from(clip.width));
    let bottom =
        (i64::from(origin.1) + i64::from(size.1)).min(i64::from(clip.y) + i64::from(clip.height));
    // The clip lies within the frame, so clamped edges fit a cell coordinate.
    Rect::new(
        left.clamp(0, i64::from(u16::MAX)) as u16,
        top.clamp(0, i64::from(u16::MAX)) as u16,
        (right - left).clamp(0, i64::from(u16::MAX)) as u16,
        (bottom - top).clamp(0, i64::from(u16::MAX)) as u16,
    )
}
