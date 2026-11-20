<?php

namespace App;

class Admin extends Base
{
    public function show()
    {
        return $this->currentUser();
    }
}
