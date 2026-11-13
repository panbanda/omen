namespace repository {

int system(const char *command) {
    return command[0];
}

int MD5(const unsigned char *data, unsigned long length) {
    return data[length];
}

int DES_ecb_encrypt(const unsigned char *input) {
    return input[0];
}

int curl_easy_setopt(void *handle, int option, long value) {
    return option + static_cast<int>(value);
}

char *strcpy(char *destination, const char *source) {
    destination[0] = source[0];
    return destination;
}

void repository_symbols(const char *command, char *buffer, const unsigned char *data, void *handle) {
    system(command);
    MD5(data, 4);
    DES_ecb_encrypt(data);
    curl_easy_setopt(handle, 64, 0L);
    strcpy(buffer, command);
}

}
