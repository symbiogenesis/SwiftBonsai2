// JSON serialization and production API dispatch only. The safe Rust suite supplies
// inputs, expected results and assertions; no upstream test source is compiled.
#include "speculative.h"
#include "unicode.h"
#include "jinja.hpp"
#include "mtmd.hpp"
#include "grammar_parser.hpp"
#include "reasoning_budget.hpp"
#include <nlohmann/json.hpp>

#include <cstdint>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <initializer_list>
#include <limits>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
using json = nlohmann::ordered_json;
constexpr std::size_t max_file_bytes = 1024 * 1024;
constexpr std::size_t max_response_bytes = 16 * 1024 * 1024;
constexpr std::size_t max_operations = 128;
constexpr std::size_t max_text_bytes = 64 * 1024;
constexpr std::size_t max_expression_bytes = 8192;
constexpr std::size_t max_expressions = 16;

void require(bool condition, const char * message) {
    if (!condition) {
        throw std::runtime_error(message);
    }
}

void require_keys(const json & object, std::initializer_list<const char *> keys) {
    require(object.is_object() && object.size() == keys.size(), "invalid object fields");
    for (const auto * key : keys) {
        require(object.contains(key), "missing or unknown object field");
    }
}

std::int32_t integer(const json & value) {
    require(value.is_number_integer(), "expected signed 32-bit integer");
    if (value.is_number_unsigned()) {
        const auto number = value.get<std::uint64_t>();
        require(number <= std::uint64_t(std::numeric_limits<std::int32_t>::max()),
                "integer outside signed 32-bit range");
        return static_cast<std::int32_t>(number);
    }
    const auto number = value.get<std::int64_t>();
    require(number >= std::numeric_limits<std::int32_t>::min() &&
            number <= std::numeric_limits<std::int32_t>::max(),
            "integer outside signed 32-bit range");
    return static_cast<std::int32_t>(number);
}

json read_request(const std::filesystem::path & path) {
    const auto size = std::filesystem::file_size(path);
    require(size > 0 && size <= max_file_bytes, "request size must be 1 byte through 1 MiB");
    std::ifstream input(path, std::ios::binary);
    require(bool(input), "cannot open request file");
    std::string content(static_cast<std::size_t>(size), '\0');
    input.read(content.data(), static_cast<std::streamsize>(content.size()));
    require(bool(input) && input.peek() == std::char_traits<char>::eof() && !input.bad(),
            "request size changed during read");
    std::vector<std::set<std::string>> object_keys;
    const auto callback = [&](int depth, json::parse_event_t event, json & parsed) {
        require(depth <= 8, "JSON nesting exceeds limit");
        if (event == json::parse_event_t::object_start) {
            object_keys.emplace_back();
        } else if (event == json::parse_event_t::key) {
            require(!object_keys.empty() && object_keys.back().insert(parsed.get<std::string>()).second,
                    "duplicate JSON object key");
        } else if (event == json::parse_event_t::object_end) {
            require(!object_keys.empty(), "invalid JSON object nesting");
            object_keys.pop_back();
        }
        return true;
    };
    return json::parse(content, callback);
}

void validate_operation(const json & operation) {
    require(operation.is_object() && operation.contains("Kind") && operation.at("Kind").is_string(),
            "operation Kind must be a string");
    const auto & kind = operation.at("Kind").get_ref<const std::string &>();
    if (kind == "unicode-regex-split") {
        require_keys(operation, {"Kind", "Text", "Expressions", "ByteEncoding"});
        require(operation.at("Text").is_string() &&
                operation.at("Text").get_ref<const std::string &>().size() <= max_text_bytes,
                "Text must be a string of at most 65536 UTF-8 bytes");
        require(operation.at("ByteEncoding").is_boolean(), "ByteEncoding must be boolean");
        const auto & expressions = operation.at("Expressions");
        require(expressions.is_array() && !expressions.empty() && expressions.size() <= max_expressions,
                "Expressions must contain 1 through 16 strings");
        for (const auto & expression : expressions) {
            require(expression.is_string() && expression.get_ref<const std::string &>().size() <= max_expression_bytes,
                    "expression must be a string of at most 8192 UTF-8 bytes");
        }
    } else if (kind == "mtp-first-decode-fits") {
        require_keys(operation, {"Kind", "Batch", "Catchup", "Sequences"});
        integer(operation.at("Batch"));
        integer(operation.at("Catchup"));
        integer(operation.at("Sequences"));
    } else if (prism_jinja::handles(kind)) {
        prism_jinja::validate(operation);
    } else if (prism_mtmd::handles(kind)) {
        prism_mtmd::validate(operation);
    } else if (prism_grammar_parser::handles(kind)) {
        prism_grammar_parser::validate(operation);
    } else if (prism_reasoning_budget::handles(kind)) {
        prism_reasoning_budget::validate(operation);
    } else {
        throw std::runtime_error("unknown operation Kind");
    }
}

json execute(const json & operation) {
    const auto & kind = operation.at("Kind").get_ref<const std::string &>();
    if (prism_jinja::handles(kind)) {
        return prism_jinja::execute(operation);
    }
    if (prism_mtmd::handles(kind)) {
        return prism_mtmd::execute(operation);
    }
    if (prism_grammar_parser::handles(kind)) {
        return prism_grammar_parser::execute(operation);
    }
    if (prism_reasoning_budget::handles(kind)) {
        return prism_reasoning_budget::execute(operation);
    }
    if (kind == "unicode-regex-split") {
        const auto pieces = unicode_regex_split(operation.at("Text").get<std::string>(),
                operation.at("Expressions").get<std::vector<std::string>>(),
                operation.at("ByteEncoding").get<bool>());
        return json{{"Pieces", pieces}};
    }
    return json{{"Fits", common_speculative_mtp_first_decode_fits(
            integer(operation.at("Batch")), integer(operation.at("Catchup")),
            integer(operation.at("Sequences")))}};
}
} // namespace

int main(int argc, char ** argv) {
    try {
        require(argc == 5 && std::strcmp(argv[1], "--input") == 0 && std::strcmp(argv[3], "--output") == 0,
                "usage: prism-upstream-common-bridge --input ABSOLUTE_PATH --output ABSOLUTE_PATH");
        const std::filesystem::path input_path(argv[2]), output_path(argv[4]);
        require(input_path.is_absolute() && output_path.is_absolute(), "protocol paths must be absolute");
        require(input_path.lexically_normal() != output_path.lexically_normal(), "input and output paths must differ");
        const auto request = read_request(input_path);
        require_keys(request, {"SchemaVersion", "Operations"});
        require(integer(request.at("SchemaVersion")) == 1, "unsupported SchemaVersion");
        const auto & operations = request.at("Operations");
        require(operations.is_array() && !operations.empty() && operations.size() <= max_operations,
                "Operations must contain 1 through 128 operations");
        for (const auto & operation : operations) {
            validate_operation(operation);
        }
        json results = json::array();
        for (const auto & operation : operations) {
            results.push_back(execute(operation));
        }
        const auto response = json{{"SchemaVersion", 1}, {"Results", results}}.dump();
        require(response.size() <= max_response_bytes, "response exceeds 16 MiB limit");
        std::ofstream output(output_path, std::ios::binary | std::ios::trunc);
        require(bool(output), "cannot open response file");
        output.exceptions(std::ios::badbit | std::ios::failbit);
        output.write(response.data(), static_cast<std::streamsize>(response.size()));
        output.close();
        return 0;
    } catch (const std::exception & error) {
        std::fprintf(stderr, "prism-upstream-common-bridge: %s\n", error.what());
        return 1;
    }
}
