<?php

namespace Repository;

class Gateway
{
    public function query($statement)
    {
        return $statement;
    }
}

function system($command)
{
    return $command;
}

function create_function($source)
{
    return $source;
}

function unserialize($blob)
{
    return $blob;
}

function md5($data)
{
    return $data;
}

function openssl_encrypt($data, $method, $key)
{
    return $data;
}

function curl_setopt($handle, $option, $value)
{
    return $handle;
}

function repository_symbols($command, $source, $blob, $name, $data, $key, $handle)
{
    system($command);
    create_function($source);
    unserialize($blob);
    $gateway = new Gateway();
    $gateway->query("SELECT * FROM users WHERE name = '" . $name . "'");
    md5($data);
    openssl_encrypt($data, "des-ecb", $key);
    curl_setopt($handle, CURLOPT_SSL_VERIFYPEER, false);
}
