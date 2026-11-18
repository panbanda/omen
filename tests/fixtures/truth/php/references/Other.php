<?php
class Other {
    public static function cleanup() {
        system('rm ' . getenv('TARGET'));
    }
}
