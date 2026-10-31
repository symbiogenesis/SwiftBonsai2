// Model-free lifetime regression for the actual generated overlay block.
// Stub graph-local allocations are independent of persistent model/KV state.
#include <cassert>
#include <cstddef>
#include <vector>

// BONSAI_DECODE_GRAPH_DECLARATIONS

// A DLL entry point receives the actual Rust exports from the owning test.
// These forwarding wrappers supply no duplicate graph-selection policy.
static size_t (*rust_index)(int, size_t) = nullptr;
static bool (*rust_rebuild)(bool, int, int) = nullptr;
extern "C" size_t bonsai_qt_decode_graph_index(int tokens, size_t windows) {
    return rust_index(tokens, windows);
}
extern "C" bool bonsai_qt_decode_graph_rebuild(bool present, int cached, int requested) {
    return rust_rebuild(present, cached, requested);
}

#ifdef NDEBUG
#error The lifetime regression requires active assertions.
#endif

static int live = 0;
static int allocations = 0;
static int releases = 0;

struct TalkerDecodeGraph {
    int * ctx = nullptr;
    int N = 0;
};

static void talker_decode_graph_free(TalkerDecodeGraph * graph) {
    if (graph->ctx) {
        delete graph->ctx;
        graph->ctx = nullptr;
        --live;
        ++releases;
    }
    graph->N = 0;
}

static bool select_graph(std::vector<TalkerDecodeGraph> & graphs, int n_kv_pad,
                         int N, bool allocate = true) {
// BONSAI_DECODE_GRAPH_SELECTION
    // Upstream width-change cleanup and allocation are unchanged by the overlay.
    if (tg->ctx && tg->N != N) {
        talker_decode_graph_free(tg);
    }
    if (!tg->ctx) {
        // Obsolete graph-local memory must be gone before replacement allocation.
        assert(live == 0);
        if (!allocate) {
            return false;
        }
        tg->ctx = new int(++allocations);
        tg->N = N;
        ++live;
    }
    assert(live == 1);
    return true;
}

extern "C" __declspec(dllexport) int bonsai_decode_graph_fixture(
    size_t (*index)(int, size_t), bool (*rebuild)(bool, int, int)) {
    rust_index = index;
    rust_rebuild = rebuild;
    assert(rust_index && rust_rebuild);
    std::vector<TalkerDecodeGraph> graphs(16);
    assert(select_graph(graphs, 256, 1));
    const int first = *graphs[0].ctx;
    assert(select_graph(graphs, 256, 1));
    assert(*graphs[0].ctx == first && allocations == 1 && releases == 0);
    // Forward windows retain only the current workspace, including the last class.
    for (int window = 512; window <= 4096; window += 256) {
        assert(select_graph(graphs, window, 1));
        assert(live == 1 && releases == allocations - 1);
    }
    // A fresh request can return to an earlier class without stale graph inputs.
    assert(select_graph(graphs, 256, 1));
    assert(*graphs[0].ctx != first && live == 1);
    // Batch-width changes rebuild the selected workspace too.
    const int before_width_change = allocations;
    assert(select_graph(graphs, 256, 2));
    assert(allocations == before_width_change + 1 && graphs[0].N == 2);
    // Failed replacement allocation frees completed scratch and permits recovery.
    assert(!select_graph(graphs, 1024, 1, false));
    assert(live == 0 && releases == allocations);
    assert(select_graph(graphs, 1024, 1));
    assert(!select_graph(graphs, 1024, 2, false));
    assert(live == 0 && releases == allocations);
    assert(select_graph(graphs, 1024, 2));
    for (auto & graph : graphs) {
        talker_decode_graph_free(&graph);
    }
    assert(live == 0 && releases == allocations);
    return 0;
}
