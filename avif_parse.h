#include <stdlib.h>

#ifdef __cplusplus
extern "C" {
#endif // __cplusplus

/**
 * Result of parsing an AVIF file. Contains AV1-compressed data.
 */
typedef struct avif_data_t {
  /**
   * AV1 data for color channels
   */
  const unsigned char *primary_data;
  size_t primary_size;
  /**
   * AV1 data for alpha channel (may be NULL if the image is fully opaque)
   */
  const unsigned char *alpha_data;
  size_t alpha_size;
  /**
   * 0 = normal, uncorrelated alpha channel
   * 1 = premultiplied alpha. You must divide RGB values by alpha.
   *
   * ```c
   * if (a != 0) {r = r * 255 / a}
   * ```
   */
  char premultiplied_alpha;
  void *reserved;
} avif_data_t;

/**
 * Parse AVIF image file and return results. Returns `NULL` if the file can't be parsed.
 *
 * Call `avif_data_free` on the result when done.
 */
const avif_data_t *avif_parse(const unsigned char *bytes, size_t bytes_len);

/**
 * Free all data related to `avif_data_t`
 */
void avif_data_free(const avif_data_t *data);

#ifdef __cplusplus
} // extern "C"
#endif // __cplusplus
