using System;

namespace App
{
    public class Service
    {
        public int Production()
        {
            return 1;
        }

        [Obsolete]
        public int TestConnection()
        {
            return Production();
        }
    }
}
