# Graphical Abstraction Manager (GAM)

The GAM provides abstract UI primitives to other modules.

The goal is to have this module work in close conjunction with the
`graphics-server`, and all other modules would route abstract UI
requests through this module.

## Structure

### Canvas

A `Canvas` is a minimal data structure that defines a physical region of the
screen that will display a set of primitives. `Canvas` structures are domiciled
in the UI server, and are considered trusted by default, although there is
a flag that can be cleared to make everything within it untrusted.

Each `Canvas` has a 128-bit GUID. Application processes that wish to draw
something to the screen must refer to a `Canvas` by its 128-bit GUID; it is up
to the GAM to not share secure GUIDs with insecure processes.

The `Canvas` selection is modal on a per-connection basis. In other words,
an application can request several `Canvas` objects to draw on, but it
must first send a `SelectCanvas` command first to pick the right `Canvas`.
By default, the last requested `Canvas` is the default `Canvas` for a
given connection to the GAM.

The region of the physical screen that can be drawn on by a `Canvas` is
defined by a `clip_rect`. The coordinate space of the `clip_rect` is fixed
to the screen's coordinates, that is, `(0,0)` in the top left, X increasing
to the right, Y increasing down.

A `Canvas` stores a `pan_offset`. If the pan offset is `(0,0)`, then the top left
corner of the `clip_rect` corresponds to data drawn at location `(0,0)` inside
the `Canvas`. The `pan_offset` is added to every coordinate inside the objects that
refer to a `Canvas` at render time; this allows for easy implementation
of panning and scrolling.

A `Canvas` has a `trust_level` associated with it. Higher numbers are more
trusted; 255 is the highest level of trust. More trusted `Canvas` objects always
render on top of lower trusted object; furthermore, when a higher trusted
`Canvas` object overlaps a lower trusted object, the lower trusted object is
defaced using hatched lines with a random angle and spacing, and further updates
are disallowed. Thus, a `Canvas` should *not* be thought of like a "window", as
windows in typical UIs are allowed to freely overlap and clipping is handled
by simply drawing over lower layers of content.

In the case that both trusted and untrusted data should be rendered on the same
screen, `Canvas` makes it strongly preferred to render them next to each other, rather
than one on top of each other. This policy is partially to help users be very
clear as to e.g. where a password box is vs. an image that looks a lot like
a password box; but the policy is also informed by the limitations of the underlying
hardware. In particular, the underlying memory LCD strongly
relies on "dirty bits" for good performance, and doing full-region redraws to
handle dirty rectangles on window movement is not an efficient use of dirty
bits. Reducing time spent redrawing partially obscured windows is also good
for performance and helps to simplify the code base, but these last two considerations
are quite minor compared to the primary concern of a "least confusing" UI when
it comes to differentiating between trustable and less trustable regions of the
screen.

Thus, the simple rule is: don't stack content types of different trust levels.
If you require content stacking, this can be done for content within a single
trust level by using multiple objects within a `Canvas`, as they have a `draw_order`
attribute and can handle content stacking; but between trust domains, it's both
a trust and complexity issue to allow for simultaneous stacking of trust domains
with live, full-content update of the underlying layers.

### TextView

A `TextView` object is a heavy data structure that contains both a `xous::String`
and metadata which guides the GAM on how to render the string. Please note
the philosophy of the GAM is to hide the exact details of how anything is
rendered to the calling application. This both allows users to have greater
control over customizing their interfaces, and also helps introduce a layer
of protection against phishing; however it also means that UX designers will not
be able to have exquisite control over the "look and feel" of their applications.

`TextView` objects are domiciled on the application process. Process-local API
calls can "simulate" certain properties (such as figuring out the dynamic
width or height of a text box based on the size of the string within) to assist
with laying out `TextViews`. Once the layout is finalized, the `TextView` objects
are then immutably lent to the GAM using a `xous::ipc::Sendable.lend()` wrapper;
the calling thread then blocks until the GAM completes the rendering operation.

One can think of a `TextView` as a text bubble, that can have rounded or square
corners, and its content string can be rendered with a selection of options
that specify the glyph style (*not* a font -- it's more of a hint than a specifier),
aligment, and the size of the text bubble. The text bubble can either be of a
fixed size (such that the string will show ellipses `...` if it overruns the
bubble), or of a dynamically growable size based on its content.

`TextView` can both be directly rendered to a `Canvas`, or managed by secondary
object such as a `Menu` or `List` to compose other UI elements.

`TextView` supports a `draw_order` attribute, which allows multiple `TextViews`
within a single application to be stacked on top of each other.
Note that `draw_order` can only be respected within the context of a single application;
if two applications are drawing to the same `Canvas`, then who gets the last
draw depend on who sends the last update request. This might be fixable later
on with a bit to "mutex" the drawing of other applications to enforce an order,
but for MVP we simply avoid having multiple applications contend for access to
the same `Canvas`.



--------------------



The problem is:
1. you don't have access to the drawing objects; they are domiciled in the apps
2. you don't want to just clear large regions of the FB; that will slow things down.
You want to take advantage of dirty bits as much as possible for good performance.
3. It would be nice to be able to have a more trusted "pop-up" on top of a less
trusted app, and have it obscure the lower app.

Options include:

1. A bitmap that stores the drawable mask based on a trust level. This would have
to be recalculated from the priority queue for each Canvas every time the queue
is updated. This structure consumes 24kiB per canvas in this case...
2. Coming up with an arbitrary polygonal region (that can also support holes) and
then doing maths to figure out if a point may or may not be updated. This would
be memory efficient but rather time and CPU-inefficient to compute.
3. Disallowing overlapping Canvas features??
4. Maintaining each Canvas with a bitmap that has the latest drawing on it,
updating only that bitmap, and then doing compositing and dirty bit computation
before sending off to the graphics engine?
