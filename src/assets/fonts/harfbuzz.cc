// Keep the HarfBuzz API boundary in C++ so Rust does not depend on generated
// enum representations, which differ between MSVC and other compilers.
#include "hb-subset.h"
#include <memory>

template <typename T, void (*Destroy)(T *)>
using Handle = std::unique_ptr<T, decltype(Destroy)>;

extern "C" hb_blob_t *typptx_hb_instantiate(
    const char *data, unsigned length, unsigned index,
    const unsigned *tags, const float *values, unsigned count) {
  Handle<hb_blob_t, hb_blob_destroy> blob(
      hb_blob_create_or_fail(data, length, HB_MEMORY_MODE_READONLY, nullptr, nullptr),
      hb_blob_destroy);
  if (!blob) return nullptr;
  Handle<hb_face_t, hb_face_destroy> face(
      hb_face_create(blob.get(), index), hb_face_destroy);
  if (!face || !hb_face_get_glyph_count(face.get())) return nullptr;
  Handle<hb_subset_input_t, hb_subset_input_destroy> input(
      hb_subset_input_create_or_fail(), hb_subset_input_destroy);
  if (!input) return nullptr;
  hb_subset_input_keep_everything(input.get());
  hb_subset_input_set_flags(input.get(),
      hb_subset_input_get_flags(input.get()) | HB_SUBSET_FLAGS_DOWNGRADE_CFF2);
  for (unsigned i = 0; i < count; ++i) {
    if (!hb_subset_input_pin_axis_location(input.get(), face.get(), tags[i], values[i]))
      return nullptr;
  }
  Handle<hb_face_t, hb_face_destroy> fixed(
      hb_subset_or_fail(face.get(), input.get()), hb_face_destroy);
  if (!fixed || hb_face_get_glyph_count(fixed.get()) != hb_face_get_glyph_count(face.get()))
    return nullptr;
  return hb_face_reference_blob(fixed.get());
}

extern "C" const char *typptx_hb_data(hb_blob_t *blob, unsigned *length) {
  return hb_blob_get_data(blob, length);
}

extern "C" void typptx_hb_destroy(hb_blob_t *blob) {
  hb_blob_destroy(blob);
}
