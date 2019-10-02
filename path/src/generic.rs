use crate::{EndpointId, CtrlPointId, PathEventId, Position, PositionStore};
use crate::events::{PathEvent, IdEvent};
use crate::math::Point;

use std::fmt;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub enum Event<Edge, Endpoint, CtrlPoint> {
    Begin { at: Endpoint, },
    Line { edge: Edge, from: Endpoint, to: Endpoint },
    Quadratic { edge: Edge, from: Endpoint, ctrl: CtrlPoint, to: Endpoint },
    Cubic { edge: Edge, from: Endpoint, ctrl1: CtrlPoint, ctrl2: CtrlPoint, to: Endpoint },
    End { edge: Edge, last: Endpoint, first: Endpoint, close: bool },
}

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
enum Verb {
    Line = 0,
    Quadratic = 1,
    Cubic = 2,
    Begin = 3,
    Close = 4,
    End = 5,
}

#[derive(Copy, Clone)]
union PathOp {
    verb: Verb,
    endpoint: EndpointId,
    ctrl_point: CtrlPointId,
    offset: u32,
}

impl PathOp {
    #[inline]
    fn get_verb(self) -> Verb {
        assert_is_verb(self);
        unsafe {
            self.verb
        }
    }
}

#[inline]
fn assert_is_verb(op: PathOp) {
    unsafe { assert!(op.offset < 6) }
}

/// The commands of a path encoded in a single array using IDs to refer
/// to endpoints and control points.
///
/// `PathCommands` is a good fit when the a custom endpoint and control point
/// types are needed or when their the user needs to control their position in
/// the buffers.
#[derive(Clone)]
#[cfg_attr(feature = "serialization", derive(Serialize, Deserialize))]
pub struct PathCommands {
    cmds: Box<[PathOp]>,
}

impl PathCommands {
    /// Creates a [PathCommandsBuilder](struct.PathCommandsBuilder.html) to create path commands.
    pub fn builder() -> PathCommandsBuilder {
        PathCommandsBuilder::new()
    }

    /// Returns an iterator over the path commands.
    pub fn id_events(&self) -> IdEvents {
        IdEvents::new(self.cmds.iter())
    }

    /// Returns a view on the path commands.
    pub fn as_slice(&self) -> PathCommandsSlice {
        PathCommandsSlice {
            cmds: &self.cmds,
        }
    }

    /// Returns a view on a path made of these commands with endpoint and
    /// control point slices.
    pub fn path_slice<'l, Endpoint, CtrlPoint>(
        &'l self,
        endpoints: &'l [Endpoint],
        ctrl_points: &'l [CtrlPoint],
    ) -> GenericPathSlice<Endpoint, CtrlPoint> {
        GenericPathSlice {
            endpoints,
            ctrl_points,
            cmds: self.as_slice(),
        }
    }

    /// Returns an iterator over the path, with endpoints and control points.
    pub fn events<'l, Endpoint, CtrlPoint>(
        &'l self,
        endpoints: &'l [Endpoint],
        ctrl_points: &'l [CtrlPoint],
    ) -> Events<Endpoint, CtrlPoint> {
        Events {
            cmds: self.cmds.iter(),
            first_endpoint: 0,
            prev_endpoint: 0,
            endpoints,
            ctrl_points,
        }
    }

    /// Returns the event for a given event ID.
    pub fn event(&self, id: PathEventId) -> IdEvent {
        self.as_slice().event(id)
    }

    /// Returns the next event id within the path.
    pub fn next_event_id_in_path(&self, id: PathEventId) -> Option<PathEventId> {
        self.as_slice().next_event_id_in_path(id)
    }

    /// Returns the next event id within the sub-path.
    ///
    /// Loops back to the first event after the end of the sub-path.
    pub fn next_event_id_in_sub_path(&self, id: PathEventId) -> PathEventId {
        self.as_slice().next_event_id_in_sub_path(id)
    }
}

impl fmt::Debug for PathCommands {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl<'l> IntoIterator for &'l PathCommands {
    type Item = IdEvent;
    type IntoIter = IdEvents<'l>;

    fn into_iter(self) -> IdEvents<'l> { self.id_events() }
}

impl<'l> Into<PathCommandsSlice<'l>> for &'l PathCommands {
    fn into(self) -> PathCommandsSlice<'l> {
        self.as_slice()
    }
}

#[derive(Copy, Clone)]
pub struct PathCommandsSlice<'l> {
    cmds: &'l [PathOp],
}

impl<'l> PathCommandsSlice<'l> {
    /// Returns an iterator over the path commands.
    pub fn id_events(&self) -> IdEvents {
        IdEvents::new(self.cmds.iter())
    }

    /// Returns the event for a given event ID.
    pub fn event(&self, id: PathEventId) -> IdEvent {
        let idx = id.to_usize();
        unsafe {
            match self.cmds[idx].get_verb() {
                Verb::Line => IdEvent::Line {
                    from: self.cmds[idx - 1].endpoint,
                    to: self.cmds[idx + 1].endpoint,
                    edge: id,
                },
                Verb::Quadratic => IdEvent::Quadratic {
                    from: self.cmds[idx - 1].endpoint,
                    ctrl: self.cmds[idx + 1].ctrl_point,
                    to: self.cmds[idx + 2].endpoint,
                    edge: id,
                },
                Verb::Cubic => IdEvent::Cubic {
                    from: self.cmds[idx - 1].endpoint,
                    ctrl1: self.cmds[idx + 1].ctrl_point,
                    ctrl2: self.cmds[idx + 2].ctrl_point,
                    to: self.cmds[idx + 3].endpoint,
                    edge: id,
                },
                Verb::Begin => IdEvent::Begin {
                    at: self.cmds[idx + 1].endpoint
                },
                Verb::End => {
                    let first_event = self.cmds[idx + 1].offset as usize;
                    IdEvent::End {
                        last: self.cmds[idx - 1].endpoint,
                        first: self.cmds[first_event + 1].endpoint,
                        close: false,
                        edge: id,
                    }
                }
                Verb::Close => {
                    let first_event = self.cmds[idx + 1].offset as usize;
                    IdEvent::End {
                        last: self.cmds[idx - 1].endpoint,
                        first: self.cmds[first_event + 1].endpoint,
                        close: true,
                        edge: id,
                    }
                }
            }
        }
    }

    /// Returns the next event id within the path.
    pub fn next_event_id_in_sub_path(&self, id: PathEventId) -> PathEventId {
        let idx = id.to_usize();
        let cmd = self.cmds[idx].get_verb();
        match cmd {
            Verb::Line | Verb::Begin => PathEventId(id.0 + 2),
            Verb::Quadratic => PathEventId(id.0 + 3),
            Verb::Cubic => PathEventId(id.0 + 4),
            Verb::End | Verb::Close => PathEventId(unsafe { self.cmds[idx + 1].offset }),
        }
    }

    /// Returns the next event id within the path.
    pub fn next_event_id_in_path(&self, id: PathEventId) -> Option<PathEventId> {
        let idx = id.to_usize();
        let next = match self.cmds[idx].get_verb() {
            Verb::Line
            | Verb::Begin
            | Verb::End
            | Verb::Close => PathEventId(id.0 + 2),
            Verb::Quadratic => PathEventId(id.0 + 3),
            Verb::Cubic => PathEventId(id.0 + 4),
        };

        if next.0 < self.cmds.len() as u32 {
            return Some(next);
        }

        None
    }
}

impl<'l> fmt::Debug for PathCommandsSlice<'l> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{ ")?;
        for evt in self.id_events() {
            match evt {
                IdEvent::Line { to, .. } => { write!(f, "L {:?}", to) }
                IdEvent::Quadratic { ctrl,  to, .. } => { write!(f, "Q {:?} {:?} ", ctrl, to) }
                IdEvent::Cubic { ctrl1, ctrl2, to, .. } => { write!(f, "C {:?} {:?} {:?} ", ctrl1, ctrl2, to) }
                IdEvent::Begin { at, .. } => { write!(f, "M {:?} ", at) }
                IdEvent::End { close: true, .. } => { write!(f, "Z ") }
                IdEvent::End { close: false, .. } => { Ok(()) }
            }?;
        }
        write!(f, "}}")
    }
}

pub struct GenericPath<Endpoint, CtrlPoint> {
    cmds: PathCommands,
    endpoints: Box<[Endpoint]>,
    ctrl_points: Box<[CtrlPoint]>,
}

impl<Endpoint, CtrlPoint> GenericPath<Endpoint, CtrlPoint> {
    /// Creates a [GenericPathBuilder](struct.GenericPathBuilder.html).
    pub fn builder() -> GenericPathBuilder<Endpoint, CtrlPoint> {
        GenericPathBuilder::new()
    }

    /// Returns a view on a path made of these commands with endpoint and
    /// control point slices.
    pub fn as_slice(&self) -> GenericPathSlice<Endpoint, CtrlPoint> {
        GenericPathSlice {
            cmds: self.cmds.as_slice(),
            endpoints: &self.endpoints,
            ctrl_points: &self.ctrl_points,
        }
    }

    /// Returns an iterator over the path commands, using endpoint
    /// and control point ids instead of positions.
    pub fn id_events(&self) -> IdEvents {
        self.cmds.id_events()
    }

    /// Returns an iterator over the path, with endpoints and control points.
    pub fn events(&self) -> Events<Endpoint, CtrlPoint> {
        Events {
            cmds: self.cmds.cmds.iter(),
            first_endpoint: 0,
            prev_endpoint: 0,
            endpoints: &self.endpoints,
            ctrl_points: &self.ctrl_points,
        }
    }

    /// Returns the event for a given event ID.
    pub fn id_event(&self, id: PathEventId) -> IdEvent {
        self.cmds.as_slice().event(id)
    }

    /// Returns the event for a given event ID.
    pub fn event(&self, id: PathEventId) -> PathEvent<&Endpoint, &CtrlPoint> {
        match self.id_event(id) {
            IdEvent::Begin { at } => PathEvent::Begin {
                at: &self[at]
            },
            IdEvent::Line { from, to, .. } => PathEvent::Line {
                from: &self[from],
                to: &self[to]
            },
            IdEvent::Quadratic { from, ctrl, to, .. } => PathEvent::Quadratic {
                from: &self[from],
                ctrl: &self[ctrl],
                to: &self[to]
            },
            IdEvent::Cubic { from, ctrl1, ctrl2, to, .. } => PathEvent::Cubic {
                from: &self[from],
                ctrl1: &self[ctrl1],
                ctrl2: &self[ctrl2],
                to: &self[to]
            },
            IdEvent::End { last, first, close, .. } => PathEvent::End {
                last: &self[last],  first: &self[first], close
            },
        }
    }

    /// Returns the next event id within the path.
    pub fn next_event_id_in_path(&self, id: PathEventId) -> Option<PathEventId> {
        self.cmds.as_slice().next_event_id_in_path(id)
    }

    /// Returns the next event id within the sub-path.
    ///
    /// Loops back to the first event after the end of the sub-path.
    pub fn next_event_id_in_sub_path(&self, id: PathEventId) -> PathEventId {
        self.cmds.as_slice().next_event_id_in_sub_path(id)
    }

    pub fn endpoints(&self) -> &[Endpoint] { &self.endpoints }

    pub fn ctrl_points(&self) -> &[CtrlPoint] { &self.ctrl_points }
}

impl<Endpoint, CtrlPoint> std::ops::Index<EndpointId> for GenericPath<Endpoint, CtrlPoint> {
    type Output = Endpoint;
    fn index(&self, id: EndpointId) -> &Endpoint {
        &self.endpoints[id.to_usize()]
    }
}

impl<Endpoint, CtrlPoint> std::ops::Index<CtrlPointId> for GenericPath<Endpoint, CtrlPoint> {
    type Output = CtrlPoint;
    fn index(&self, id: CtrlPointId) -> &CtrlPoint {
        &self.ctrl_points[id.to_usize()]
    }
}

/// A view on a `Path`.
#[derive(Copy, Clone)]
pub struct GenericPathSlice<'l, Endpoint, CtrlPoint> {
    endpoints: &'l [Endpoint],
    ctrl_points: &'l [CtrlPoint],
    cmds: PathCommandsSlice<'l>,
}

impl<'l, Endpoint, CtrlPoint> GenericPathSlice<'l, Endpoint, CtrlPoint> {
    /// Returns an iterator over the events of the path using IDs.
    pub fn id_events(&self) -> IdEvents {
        self.cmds.id_events()
    }

    /// Returns an iterator over the events of the path using endpoint
    /// and control point references.
    pub fn events(&self) -> Events<Endpoint, CtrlPoint> {
        Events {
            cmds: self.cmds.cmds.iter(),
            first_endpoint: 0,
            prev_endpoint: 0,
            endpoints: &self.endpoints[..],
            ctrl_points: &self.ctrl_points[..],
        }
    }
}

impl<'l, Endpoint, CtrlPoint> std::ops::Index<EndpointId> for GenericPathSlice<'l, Endpoint, CtrlPoint> {
    type Output = Endpoint;
    fn index(&self, id: EndpointId) -> &Endpoint {
        &self.endpoints[id.to_usize()]
    }
}

impl<'l, Endpoint, CtrlPoint> std::ops::Index<CtrlPointId> for GenericPathSlice<'l, Endpoint, CtrlPoint> {
    type Output = CtrlPoint;
    fn index(&self, id: CtrlPointId) -> &CtrlPoint {
        &self.ctrl_points[id.to_usize()]
    }
}

impl<'l, Endpoint, CtrlPoint> fmt::Debug for GenericPathSlice<'l, Endpoint, CtrlPoint>
where
    Endpoint: fmt::Debug,
    CtrlPoint: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{{ ")?;
        for evt in self.events() {
            match evt {
                PathEvent::Line { to, .. } => { write!(f, "L {:?}", to) }
                PathEvent::Quadratic { ctrl,  to, .. } => { write!(f, "Q {:?} {:?} ", ctrl, to) }
                PathEvent::Cubic { ctrl1, ctrl2, to, .. } => { write!(f, "C {:?} {:?} {:?} ", ctrl1, ctrl2, to) }
                PathEvent::Begin { at, .. } => { write!(f, "M {:?} ", at) }
                PathEvent::End { close: true, .. } => { write!(f, "Z ") }
                PathEvent::End { close: false, .. } => { Ok(()) }
            }?;
        }
        write!(f, "}}")
    }
}

/// Builds path commands.
#[derive(Clone)]
pub struct PathCommandsBuilder {
    cmds: Vec<PathOp>,
    last_cmd: Verb,
    start: u32,
    first_event_index: u32,
}

impl PathCommandsBuilder {
    /// Creates a builder without allocating memory.
    pub fn new() -> Self {
        Self {
            start: 0,
            cmds: Vec::new(),
            last_cmd: Verb::End,
            first_event_index: 0,
        }
    }

    /// Creates a pre-allocated builder.
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            start: 0,
            cmds: Vec::with_capacity(cap),
            last_cmd: Verb::End,
            first_event_index: 0,
        }
    }

    pub fn move_to(&mut self, to: EndpointId) -> PathEventId {
        self.end_if_needed();
        self.first_event_index = self.cmds.len() as u32;
        let id = PathEventId(self.cmds.len() as u32);
        self.cmds.push(PathOp { verb: Verb::Begin });
        self.cmds.push(PathOp { endpoint: to });
        self.last_cmd = Verb::Begin;

        id
    }

    pub fn line_to(&mut self, to: EndpointId) -> PathEventId {
        self.begin_if_needed();
        let id = PathEventId(self.cmds.len() as u32);
        self.cmds.push(PathOp { verb: Verb::Line });
        self.cmds.push(PathOp { endpoint: to });
        self.last_cmd = Verb::Line;

        id
    }

    pub fn quadratic_bezier_to(&mut self, ctrl: CtrlPointId, to: EndpointId) -> PathEventId {
        self.begin_if_needed();
        let id = PathEventId(self.cmds.len() as u32);
        self.cmds.push(PathOp { verb: Verb::Quadratic });
        self.cmds.push(PathOp { ctrl_point: ctrl });
        self.cmds.push(PathOp { endpoint: to });
        self.last_cmd = Verb::Quadratic;

        id
    }

    pub fn cubic_bezier_to(&mut self, ctrl1: CtrlPointId, ctrl2: CtrlPointId, to: EndpointId) -> PathEventId {
        self.begin_if_needed();
        let id = PathEventId(self.cmds.len() as u32);
        self.cmds.push(PathOp { verb: Verb::Cubic });
        self.cmds.push(PathOp { ctrl_point: ctrl1 });
        self.cmds.push(PathOp { ctrl_point: ctrl2 });
        self.cmds.push(PathOp { endpoint: to });
        self.last_cmd = Verb::Cubic;

        id
    }

    pub fn close(&mut self) -> PathEventId {
        let id = PathEventId(self.cmds.len() as u32);
        match self.last_cmd {
            Verb::Close | Verb::End => {
                return id;
            }
            _ => {}
        }
        self.cmds.push(PathOp { verb: Verb::Close });
        self.cmds.push(PathOp { offset: self.first_event_index });
        self.last_cmd = Verb::Close;

        id
    }

    fn begin_if_needed(&mut self) {
        match self.last_cmd {
            Verb::Close | Verb::End => {
                let first = self.cmds.last().cloned().unwrap_or(PathOp { offset: 0 });
                self.move_to(unsafe { first.endpoint });
            }
            _ => {}
        }
    }

    fn end_if_needed(&mut self) {
        match self.last_cmd {
            Verb::Line | Verb::Quadratic | Verb::Cubic => {
                self.cmds.push(PathOp { verb: Verb::End });
                self.cmds.push(PathOp { offset: self.first_event_index });
            }
            _ => {}
        }
    }

    /// Consumes the builder and returns path commands.
    pub fn build(mut self) -> PathCommands {
        self.end_if_needed();

        PathCommands {
            cmds: self.cmds.into_boxed_slice(),
        }
    }
}

/// Builds path commands as well as endpoint and control point vectors.
pub struct GenericPathBuilder<Endpoint, CtrlPoint> {
    endpoints: Vec<Endpoint>,
    ctrl_points: Vec<CtrlPoint>,
    cmds: PathCommandsBuilder,
}

impl<Endpoint, CtrlPoint> GenericPathBuilder<Endpoint, CtrlPoint> {
    /// Creates a builder without allocating memory.
    pub fn new() -> Self {
        Self {
            endpoints: Vec::new(),
            ctrl_points: Vec::new(),
            cmds: PathCommandsBuilder::new(),
        }
    }

    /// Creates a pre-allocated builder.
    pub fn with_capacity(
        n_endpoints: usize,
        n_ctrl_points: usize,
        n_edges: usize,
    ) -> Self {
        Self {
            endpoints: Vec::with_capacity(n_endpoints),
            ctrl_points: Vec::with_capacity(n_ctrl_points),
            cmds: PathCommandsBuilder::with_capacity(n_edges + n_endpoints + n_ctrl_points),
        }
    }

    pub fn move_to(&mut self, to: Endpoint) -> PathEventId {
        let id = self.add_endpoint(to);
        self.cmds.move_to(id)
    }

    pub fn line_to(&mut self, to: Endpoint) -> PathEventId {
        let id = self.add_endpoint(to);
        self.cmds.line_to(id)
    }

    pub fn quadratic_bezier_to(&mut self, ctrl: CtrlPoint, to: Endpoint) -> PathEventId {
        let ctrl = self.add_ctrl_point(ctrl);
        let to = self.add_endpoint(to);
        self.cmds.quadratic_bezier_to(ctrl, to)
    }

    pub fn cubic_bezier_to(&mut self, ctrl1: CtrlPoint, ctrl2: CtrlPoint, to: Endpoint) -> PathEventId {
        let ctrl1 = self.add_ctrl_point(ctrl1);
        let ctrl2 = self.add_ctrl_point(ctrl2);
        let to = self.add_endpoint(to);
        self.cmds.cubic_bezier_to(ctrl1, ctrl2, to)
    }

    pub fn close(&mut self) -> PathEventId {
        self.cmds.close()
    }

    /// Consumes the builder and returns the generated path commands.
    pub fn build(self) -> GenericPath<Endpoint, CtrlPoint> {
        GenericPath {
            cmds: self.cmds.build(),
            endpoints: self.endpoints.into_boxed_slice(),
            ctrl_points: self.ctrl_points.into_boxed_slice(),
        }
    }

    #[inline]
    fn add_endpoint(&mut self, ep: Endpoint) -> EndpointId {
        let id = EndpointId(self.endpoints.len() as u32);
        self.endpoints.push(ep);
        id
    }

    #[inline]
    fn add_ctrl_point(&mut self, cp: CtrlPoint) -> CtrlPointId {
        let id = CtrlPointId(self.ctrl_points.len() as u32);
        self.ctrl_points.push(cp);
        id
    }
}

/// An iterator of `PathEvent<&Endpoint, &CtrlPoint>`.
#[derive(Clone)]
pub struct Events<'l, Endpoint, CtrlPoint> {
    cmds: std::slice::Iter<'l, PathOp>,
    prev_endpoint: usize,
    first_endpoint: usize,
    endpoints: &'l[Endpoint],
    ctrl_points: &'l[CtrlPoint],
}

impl<'l, Endpoint, CtrlPoint> Iterator for Events<'l, Endpoint, CtrlPoint> {
    type Item = PathEvent<&'l Endpoint, &'l CtrlPoint>;

    fn next(&mut self) -> Option<PathEvent<&'l Endpoint, &'l CtrlPoint>> {
        unsafe {
            match self.cmds.next() {
                Some(&PathOp { verb: Verb::Begin }) => {
                    let to = self.cmds.next().unwrap().offset as usize;
                    self.prev_endpoint = to;
                    self.first_endpoint = to;
                    Some(PathEvent::Begin {
                        at: &self.endpoints[to]
                    })
                }
                Some(&PathOp { verb: Verb::Line }) => {
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Line {
                        from: &self.endpoints[from],
                        to: &self.endpoints[to],
                    })
                }
                Some(&PathOp { verb: Verb::Quadratic }) => {
                    let ctrl = self.cmds.next().unwrap().offset as usize;
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Quadratic {
                        from: &self.endpoints[from],
                        ctrl: &self.ctrl_points[ctrl],
                        to: &self.endpoints[to],
                    })
                }
                Some(&PathOp { verb: Verb::Cubic }) => {
                    let ctrl1 = self.cmds.next().unwrap().offset as usize;
                    let ctrl2 = self.cmds.next().unwrap().offset as usize;
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Cubic {
                        from: &self.endpoints[from],
                        ctrl1: &self.ctrl_points[ctrl1],
                        ctrl2: &self.ctrl_points[ctrl2],
                        to: &self.endpoints[to],
                    })
                }
                Some(&PathOp { verb: Verb::End }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    Some(PathEvent::End {
                        last: &self.endpoints[last],
                        first: &self.endpoints[first],
                        close: false,
                    })
                }
                Some(&PathOp { verb: Verb::Close }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    Some(PathEvent::End {
                        last: &self.endpoints[last],
                        first: &self.endpoints[first],
                        close: true,
                    })
                }
                _ => None,
            }
        }
    }
}

impl<'l, Ep, Cp> Events<'l, Ep, Cp>
where
    Ep: Position,
    Cp: Position,
{
    pub fn points(self) -> PointEvents<'l, Ep, Cp> {
        PointEvents {
            cmds: self.cmds,
            prev_endpoint: self.prev_endpoint,
            first_endpoint: self.first_endpoint,
            endpoints: self.endpoints,
            ctrl_points: self.ctrl_points,
        }
    }
}
/// An iterator of `PathEvent<&Endpoint, &CtrlPoint>`.
#[derive(Clone)]
pub struct IdEvents<'l> {
    cmds: std::slice::Iter<'l, PathOp>,
    idx: u32,
    prev_endpoint: EndpointId,
    first_endpoint: EndpointId,
}

impl<'l> IdEvents<'l> {
    fn new(cmds: std::slice::Iter<'l, PathOp>) -> Self {
        IdEvents {
            cmds,
            idx:0,
            prev_endpoint: EndpointId(0),
            first_endpoint: EndpointId(0),
        }
    }
}

impl<'l> Iterator for IdEvents<'l> {
    type Item = IdEvent;

    fn next(&mut self) -> Option<IdEvent> {
        let evt_idx = PathEventId(self.idx);

        unsafe {
            match self.cmds.next() {
                Some(&PathOp { verb: Verb::Begin }) => {
                    let to = self.cmds.next().unwrap().endpoint;
                    self.prev_endpoint = to;
                    self.first_endpoint = to;
                    self.idx += 2;
                    Some(IdEvent::Begin { at: to })
                }
                Some(&PathOp { verb: Verb::Line }) => {
                    let to = self.cmds.next().unwrap().endpoint;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    self.idx += 2;
                    Some(IdEvent::Line {
                        from: from,
                        to: to,
                        edge: evt_idx,
                    })
                }
                Some(&PathOp { verb: Verb::Quadratic }) => {
                    let ctrl = self.cmds.next().unwrap().ctrl_point;
                    let to = self.cmds.next().unwrap().endpoint;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    self.idx += 3;
                    Some(IdEvent::Quadratic {
                        from: from,
                        ctrl: ctrl,
                        to: to,
                        edge: evt_idx,
                    })
                }
                Some(&PathOp { verb: Verb::Cubic }) => {
                    let ctrl1 = self.cmds.next().unwrap().ctrl_point;
                    let ctrl2 = self.cmds.next().unwrap().ctrl_point;
                    let to = self.cmds.next().unwrap().endpoint;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    self.idx += 4;
                    Some(IdEvent::Cubic {
                        from: from,
                        ctrl1: ctrl1,
                        ctrl2: ctrl2,
                        to: to,
                        edge: evt_idx,
                    })
                }
                Some(&PathOp { verb: Verb::End }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    self.idx += 2;
                    Some(IdEvent::End {
                        last: last,
                        first: first,
                        close: false,
                        edge: evt_idx,
                    })
                }
                Some(&PathOp { verb: Verb::Close }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    self.idx += 2;
                    Some(IdEvent::End {
                        last: last,
                        first: first,
                        close: true,
                        edge: evt_idx,
                    })
                }
                _ => None,
            }
        }
    }
}

/// An iterator of `PathEvent<Point, Point>`.
#[derive(Clone)]
pub struct PointEvents<'l, Endpoint, CtrlPoint> {
    cmds: std::slice::Iter<'l, PathOp>,
    prev_endpoint: usize,
    first_endpoint: usize,
    endpoints: &'l[Endpoint],
    ctrl_points: &'l[CtrlPoint],
}

impl<'l, Endpoint, CtrlPoint> Iterator for PointEvents<'l, Endpoint, CtrlPoint>
where
    Endpoint: Position,
    CtrlPoint: Position,
{
    type Item = PathEvent<Point, Point>;

    fn next(&mut self) -> Option<PathEvent<Point, Point>> {
        unsafe {
            match self.cmds.next() {
                Some(&PathOp { verb: Verb::Begin }) => {
                    let to = self.cmds.next().unwrap().offset as usize;
                    self.prev_endpoint = to;
                    self.first_endpoint = to;
                    Some(PathEvent::Begin {
                        at: self.endpoints[to].position()
                    })
                }
                Some(&PathOp { verb: Verb::Line }) => {
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Line {
                        from: self.endpoints[from].position(),
                        to: self.endpoints[to].position(),
                    })
                }
                Some(&PathOp { verb: Verb::Quadratic }) => {
                    let ctrl = self.cmds.next().unwrap().offset as usize;
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Quadratic {
                        from: self.endpoints[from].position(),
                        ctrl: self.ctrl_points[ctrl].position(),
                        to: self.endpoints[to].position(),
                    })
                }
                Some(&PathOp { verb: Verb::Cubic }) => {
                    let ctrl1 = self.cmds.next().unwrap().offset as usize;
                    let ctrl2 = self.cmds.next().unwrap().offset as usize;
                    let to = self.cmds.next().unwrap().offset as usize;
                    let from = self.prev_endpoint;
                    self.prev_endpoint = to;
                    Some(PathEvent::Cubic {
                        from: self.endpoints[from].position(),
                        ctrl1: self.ctrl_points[ctrl1].position(),
                        ctrl2: self.ctrl_points[ctrl2].position(),
                        to: self.endpoints[to].position(),
                    })
                }
                Some(&PathOp { verb: Verb::End }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    Some(PathEvent::End {
                        last: self.endpoints[last].position(),
                        first: self.endpoints[first].position(),
                        close: false,
                    })
                }
                Some(&PathOp { verb: Verb::Close }) => {
                    let _first_index = self.cmds.next();
                    let last = self.prev_endpoint;
                    let first = self.first_endpoint;
                    self.prev_endpoint = first;
                    Some(PathEvent::End {
                        last: self.endpoints[last].position(),
                        first: self.endpoints[first].position(),
                        close: true,
                    })
                }
                _ => None,
            }
        }
    }
}

impl<'l, Endpoint, CtrlPoint> PositionStore for GenericPathSlice<'l, Endpoint, CtrlPoint>
where
    Endpoint: Position,
    CtrlPoint: Position,
{
    fn endpoint_position(&self, id: EndpointId) -> Point {
        self[id].position()
    }

    fn ctrl_point_position(&self, id: CtrlPointId) -> Point {
        self[id].position()
    }
}

impl<Endpoint, CtrlPoint> PositionStore for GenericPath<Endpoint, CtrlPoint>
where
    Endpoint: Position,
    CtrlPoint: Position,
{
    fn endpoint_position(&self, id: EndpointId) -> Point {
        self[id].position()
    }

    fn ctrl_point_position(&self, id: CtrlPointId) -> Point {
        self[id].position()
    }
}


#[test]
fn simple_path() {
    let mut builder = PathCommands::builder();
    builder.move_to(EndpointId(0));
    builder.line_to(EndpointId(1));
    builder.quadratic_bezier_to(CtrlPointId(2), EndpointId(3));
    builder.cubic_bezier_to(CtrlPointId(4), CtrlPointId(5), EndpointId(6));

    builder.move_to(EndpointId(10));
    builder.line_to(EndpointId(11));
    builder.quadratic_bezier_to(CtrlPointId(12), EndpointId(13));
    builder.cubic_bezier_to(CtrlPointId(14), CtrlPointId(15), EndpointId(16));
    builder.close();

    builder.move_to(EndpointId(20));
    builder.line_to(EndpointId(21));
    builder.quadratic_bezier_to(CtrlPointId(22), EndpointId(23));
    builder.cubic_bezier_to(CtrlPointId(24), CtrlPointId(25), EndpointId(26));

    let path = builder.build();
    let mut iter = path.id_events();
    assert_eq!(iter.next(), Some(IdEvent::Begin { at: EndpointId(0) }));
    assert_eq!(iter.next(), Some(IdEvent::Line { from: EndpointId(0), to: EndpointId(1), edge: PathEventId(2) }));
    assert_eq!(iter.next(), Some(IdEvent::Quadratic { from: EndpointId(1), ctrl: CtrlPointId(2), to: EndpointId(3), edge: PathEventId(4) }));
    assert_eq!(iter.next(), Some(IdEvent::Cubic { from: EndpointId(3), ctrl1: CtrlPointId(4), ctrl2: CtrlPointId(5), to: EndpointId(6), edge: PathEventId(7) }));
    assert_eq!(iter.next(), Some(IdEvent::End { last: EndpointId(6), first: EndpointId(0), close: false, edge: PathEventId(11) }));

    assert_eq!(iter.next(), Some(IdEvent::Begin { at: EndpointId(10) }));
    assert_eq!(iter.next(), Some(IdEvent::Line { from: EndpointId(10), to: EndpointId(11), edge: PathEventId(15) }));
    assert_eq!(iter.next(), Some(IdEvent::Quadratic { from: EndpointId(11), ctrl: CtrlPointId(12), to: EndpointId(13), edge: PathEventId(17) }));
    assert_eq!(iter.next(), Some(IdEvent::Cubic { from: EndpointId(13), ctrl1: CtrlPointId(14), ctrl2: CtrlPointId(15), to: EndpointId(16), edge: PathEventId(20) }));
    assert_eq!(iter.next(), Some(IdEvent::End { last: EndpointId(16), first: EndpointId(10), close: true, edge: PathEventId(24) }));

    assert_eq!(iter.next(), Some(IdEvent::Begin { at: EndpointId(20) }));
    assert_eq!(iter.next(), Some(IdEvent::Line { from: EndpointId(20), to: EndpointId(21), edge: PathEventId(28) }));
    assert_eq!(iter.next(), Some(IdEvent::Quadratic { from: EndpointId(21), ctrl: CtrlPointId(22), to: EndpointId(23), edge: PathEventId(30) }));
    assert_eq!(iter.next(), Some(IdEvent::Cubic { from: EndpointId(23), ctrl1: CtrlPointId(24), ctrl2: CtrlPointId(25), to: EndpointId(26), edge: PathEventId(33) }));
    assert_eq!(iter.next(), Some(IdEvent::End { last: EndpointId(26), first: EndpointId(20), close: false, edge: PathEventId(37) }));

    assert_eq!(iter.next(), None);
}

#[test]
fn next_event() {
    let mut builder = PathCommands::builder();
    builder.move_to(EndpointId(0));
    builder.line_to(EndpointId(1));
    builder.quadratic_bezier_to(CtrlPointId(2), EndpointId(3));
    builder.cubic_bezier_to(CtrlPointId(4), CtrlPointId(5), EndpointId(6));

    builder.move_to(EndpointId(10));
    builder.line_to(EndpointId(11));
    builder.quadratic_bezier_to(CtrlPointId(12), EndpointId(13));
    builder.cubic_bezier_to(CtrlPointId(14), CtrlPointId(15), EndpointId(16));
    builder.close();

    builder.move_to(EndpointId(20));
    builder.line_to(EndpointId(21));
    builder.quadratic_bezier_to(CtrlPointId(22), EndpointId(23));
    builder.cubic_bezier_to(CtrlPointId(24), CtrlPointId(25), EndpointId(26));

    let path = builder.build();

    let mut id = PathEventId(0);
    let first = id;
    assert_eq!(path.event(id), IdEvent::Begin { at: EndpointId(0) });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Line { from: EndpointId(0), to: EndpointId(1), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Quadratic { from: EndpointId(1), ctrl: CtrlPointId(2), to: EndpointId(3), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Cubic { from: EndpointId(3), ctrl1: CtrlPointId(4), ctrl2: CtrlPointId(5), to: EndpointId(6), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::End { last: EndpointId(6), first: EndpointId(0), close: false, edge: id });

    assert_eq!(path.next_event_id_in_sub_path(id), first);

    id = path.next_event_id_in_path(id).unwrap();
    let first = id;
    assert_eq!(path.event(id), IdEvent::Begin { at: EndpointId(10) });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Line { from: EndpointId(10), to: EndpointId(11), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Quadratic { from: EndpointId(11), ctrl: CtrlPointId(12), to: EndpointId(13), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Cubic { from: EndpointId(13), ctrl1: CtrlPointId(14), ctrl2: CtrlPointId(15), to: EndpointId(16), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::End { last: EndpointId(16), first: EndpointId(10), close: true, edge: id });

    assert_eq!(path.next_event_id_in_sub_path(id), first);

    id = path.next_event_id_in_path(id).unwrap();
    let first = id;
    assert_eq!(path.event(id), IdEvent::Begin { at: EndpointId(20) });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Line { from: EndpointId(20), to: EndpointId(21), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Quadratic { from: EndpointId(21), ctrl: CtrlPointId(22), to: EndpointId(23), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::Cubic { from: EndpointId(23), ctrl1: CtrlPointId(24), ctrl2: CtrlPointId(25), to: EndpointId(26), edge: id });
    id = path.next_event_id_in_path(id).unwrap();
    assert_eq!(path.event(id), IdEvent::End { last: EndpointId(26), first: EndpointId(20), close: false, edge: id });

    assert_eq!(path.next_event_id_in_path(id), None);
    assert_eq!(path.next_event_id_in_sub_path(id), first);
}