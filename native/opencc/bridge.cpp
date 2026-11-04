// The only native dependency in the literal matcher is the exact v4 OpenCC converter.
#include "Config.hpp"
#include "Converter.hpp"
#include "ResourceProvider.hpp"
#include <array>
#include <exception>
#include <memory>
#include <stdexcept>
#include <string>
#include <string_view>
#include <unordered_map>

extern "C" {
struct Span { const char *data; size_t size; };
typedef void (*WriteText)(void *, const char *, size_t);
}

namespace {
constexpr std::array<const char *, 4> names = {
    "CJK_Compatibility_Ideographs.ocd2", "TSPhrases.ocd2", "TSCharactersExt.ocd2", "TSCharacters.ocd2"
};
class Embedded final : public opencc::ResourceProvider {
    std::unordered_map<std::string, std::shared_ptr<const Resource>> files;
public:
    explicit Embedded(const Span *dicts) {
        for (size_t i=0; i<names.size(); ++i) {
            std::string name(names[i]);
            files.emplace(name,std::make_shared<Resource>(name,dicts[i].data,dicts[i].size,nullptr,"yuki-t2s-1.4.2:"+name));
        }
    }
    std::string Resolve(std::string_view name) const override {
        if (!files.count(std::string(name))) throw std::runtime_error("unknown embedded dictionary");
        return std::string(name);
    }
    std::shared_ptr<const Resource> GetResource(std::string_view name) const override {
        return files.at(std::string(name));
    }
};
}
extern "C" int yuki_opencc_t2s(Span config, const Span *dicts, Span input, void *context, WriteText write) noexcept {
    try {
        static const auto converter=opencc::Config().NewFromString(
            std::string(config.data,config.size),std::make_shared<Embedded>(dicts));
        const auto text=converter->Convert(std::string_view(input.data,input.size));
        write(context,text.data(),text.size());
        return 0;
    } catch (const std::exception &) {return 1;}
    catch (...) {return 2;}
}
