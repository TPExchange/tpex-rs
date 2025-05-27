#include "../tpex-capi.h"

int main() {
    tpex_state* state = tpex_new();
    tpex_replay(state, "{\"id\":1,\"time\":\"2024-03-28T21:34:50.364011290Z\",\"action\":{\"UpdateBankers\":{\"bankers\":[\"1\",\"2\"],\"banker\":\"bank\"}}}");

}
