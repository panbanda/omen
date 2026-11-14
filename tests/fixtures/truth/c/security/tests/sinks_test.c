#include <curl/curl.h>
#include <openssl/des.h>
#include <openssl/md5.h>
#include <stdlib.h>
#include <string.h>

void test_everything(const char *command, char *buffer, const char *source, const unsigned char *data, unsigned char *digest, CURL *handle, const_DES_cblock *input, DES_cblock *output, DES_key_schedule *schedule) {
    system(command);
    MD5(data, 4, digest);
    DES_ecb_encrypt(input, output, schedule, DES_ENCRYPT);
    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 0L);
    const char *token = "AKIAJ3KD92LQ7MZP5XWB";
    strcpy(buffer, source);
}
