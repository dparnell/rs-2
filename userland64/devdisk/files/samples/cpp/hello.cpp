// C++20 with the standard library.
#include <algorithm>
#include <iostream>
#include <stdexcept>
#include <string>
#include <vector>

int main() {
    std::vector<std::string> words{"rs2", "on", "C++", "Hello"};
    std::ranges::reverse(words);
    for (const auto &w : words)
        std::cout << w << ' ';
    std::cout << std::endl;
    try {
        throw std::runtime_error("exceptions work");
    } catch (const std::exception &e) {
        std::cout << e.what() << std::endl;
    }
}
