#include <iostream>
#include <vector>
#define STR_(x) #x
#define STR(x) STR_(x)
int main() {
    std::vector<int> v{1, 2};
    std::cout << STR(WHAT) << " " << v.size() << std::endl;
}
