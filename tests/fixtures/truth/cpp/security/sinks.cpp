#include <cstdlib>
#include <cstring>
#include <curl/curl.h>
#include <openssl/des.h>
#include <openssl/md5.h>
#include <openssl/sha.h>
#include <unistd.h>

namespace stdlib = std;

void command_execution(const char *command) {
    std::system(command);
    stdlib::system(command);
    getpid();
    const char *text = "std::system(command)";
    // std::system(command)
}

void weak_hash(const unsigned char *data, unsigned long length, unsigned char *digest) {
    MD5(data, length, digest);
    SHA256(data, length, digest);
    const char *text = "MD5(data, length, digest)";
    // MD5(data, length, digest)
}

void weak_cipher(const_DES_cblock *input, DES_cblock *output, DES_key_schedule *schedule) {
    DES_ecb_encrypt(input, output, schedule, DES_ENCRYPT);
    DES_set_odd_parity(output);
    const char *text = "DES_ecb_encrypt(input, output, schedule)";
    // DES_ecb_encrypt(input, output, schedule)
}

void tls_verification_disabled(CURL *handle) {
    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 0L);
    curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 1L);
    const char *text = "curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 0L)";
    // curl_easy_setopt(handle, CURLOPT_SSL_VERIFYPEER, 0L)
}

void hardcoded_credential() {
    const char *token = "AKIAJ3KD92LQ7MZP5XWB";
    const char *name = "AKIAEXAMPLE";
    // AKIAJ3KD92LQ7MZP5XWB
}

void memory_unsafe(char *buffer, const char *source, size_t size) {
    strcpy(buffer, source);
    stdlib::strcat(buffer, source);
    strncpy(buffer, source, size);
    const char *text = "strcpy(buffer, source)";
    // strcpy(buffer, source)
}
