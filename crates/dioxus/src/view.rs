//! Component ownership and event routing into the core tree.
use crate::host;
use crate::{Error, Registry};
use dioxus_core::{Event as UiEvent, VirtualDom};
use std::{any::Any, rc::Rc};
use wove::{Buffer, Dispatch, Event, Tree};

/// Owns a Dioxus application and its terminal nodes. The caller owns scheduling.
pub struct View {
    dom: VirtualDom,
    host: host::Host,
}
impl View {
    pub fn new(dom: VirtualDom) -> Result<Self, Error> {
        Self::with_registry(dom, Registry::default())
    }
    pub fn with_registry(mut dom: VirtualDom, registry: Registry) -> Result<Self, Error> {
        let mut host = host::Host::new(registry);
        dom.rebuild(&mut host);
        host.check()?;
        Ok(Self { dom, host })
    }
    pub fn tree(&self) -> &Tree {
        &self.host.tree
    }
    pub fn focus(&mut self, id: Option<wove::Id>) -> Result<(), Error> {
        self.host.check()?;
        self.host.tree.focus(id)?;
        Ok(())
    }
    pub fn focus_next(&mut self, reverse: bool) -> Result<(), Error> {
        self.host.check()?;
        self.host.tree.focus_next(reverse)?;
        Ok(())
    }
    /// Apply scheduled component updates. A failed mutation makes the view unusable.
    pub fn render(&mut self) -> Result<(), Error> {
        self.host.check()?;
        self.dom.render_immediate(&mut self.host);
        self.host.check()
    }
    pub fn frame(&mut self, width: u16, height: u16) -> Result<&Buffer, Error> {
        self.render()?;
        Ok(self.host.tree.frame(width, height)?)
    }
    /// Dispatch to Dioxus before native element behavior. `prevent_default` cancels
    /// editing or focus traversal; `stop_propagation` stops Dioxus parent listeners.
    pub fn send(&mut self, event: Event) -> Result<Dispatch, Error> {
        self.render()?;
        let target = self.host.tree.target(&event);
        let name = match event {
            Event::Key(..) => "key",
            Event::Paste(..) => "paste",
            Event::Mouse(..) => "mouse",
            Event::Focus => "focus",
            Event::Blur => "blur",
            Event::Enter => "enter",
            Event::Leave => "leave",
            Event::WindowFocus(..) => "window",
            Event::Resize(..) => "resize",
        };
        if !self.emit(target, name, Rc::new(event.clone())) {
            self.render()?;
            return Ok(Dispatch {
                target,
                handled: true,
                ..Dispatch::default()
            });
        }
        let input = std::iter::successors(target, |id| self.host.tree.parent(*id))
            .find_map(|id| self.input_value(id).map(|value| (id, value.to_owned())));
        let result = self.host.tree.dispatch(event)?;
        if let Some((id, before)) = input {
            if let Some(value) = self.input_value(id) {
                if value != before {
                    self.emit(Some(id), "input", Rc::new(value.to_owned()));
                }
            }
        }
        self.render()?;
        Ok(result)
    }
    /// Both editable elements share the same value-change event.
    fn input_value(&self, id: wove::Id) -> Option<&str> {
        use wove::elements::{Input, Textarea};
        self.host
            .tree
            .get::<Input>(id)
            .map(|input| input.editor.text())
            .ok()
            .or_else(|| {
                self.host
                    .tree
                    .get::<Textarea>(id)
                    .map(|area| area.editor.text())
                    .ok()
            })
    }
    fn emit(&self, target: Option<wove::Id>, name: &str, data: Rc<dyn Any>) -> bool {
        let mut node = target;
        while let Some(id) = node {
            if let Some(element) = self.host.listener(id, name) {
                let ui = UiEvent::new(data, true);
                self.dom.runtime().handle_event(name, ui.clone(), element);
                return ui.default_action_enabled();
            }
            node = self.host.tree.parent(id);
        }
        true
    }
    /// Wait for a signal or task to schedule work. Combine with terminal input in
    /// the application's executor; the adapter does not impose a runtime.
    pub async fn wait_for_work(&mut self) {
        self.dom.wait_for_work().await;
    }
}
